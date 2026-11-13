# egui_dockspace

Docking layouts for egui, backed by the renderer-neutral `dockspace` core.

Add tabs, splitters, docking guides and floating groups. Keep pane state in your
application and render it through `PaneView`. The adapter supplies egui styling,
keyboard/AccessKit actions and application-controlled close requests.

**Pre-release:** use a path dependency from this checkout. Requires Rust 1.95
and egui 0.37. Native OS windows use a separate experimental adapter.

## Run the workbench

From the repository root:

```sh
cargo run -p egui_dockspace --example workbench --features wgpu --locked
```

This official-egui/WGPU host supports drag docking, splitter resizing and
floating groups in one OS window. Start with
[`workbench/editor.rs`](examples/workbench/editor.rs) for layout, pane rendering,
styling and close decisions. The example uses the
[public winit integration](src/winit.rs) for complete input and presentation.

Drag tabs onto a pane to merge or split it. To float a tab, hold **Shift** while
dragging and release inside the window. Shift temporarily disables docking. Drag a
floating group's dotted title grip onto a docking guide to dock the group again.

## Use winit

For a small runnable editor with full drag docking:

```sh
cargo run -p egui_dockspace --example winit --features wgpu --locked
```

Enable only the integration your application needs; platform features are off by default:

```toml
[dependencies]
dockspace = { path = "../dockspace/crates/dockspace" }
egui_dockspace = { path = "../dockspace/crates/egui_dockspace", features = ["wgpu"] }
egui = "=0.37.0"
```

- `winit` exposes `egui_dockspace::winit::WinitIntegration` for an existing
  event loop and renderer. Feed window events, take input, run Dockspace, and
  handle platform output. Your renderer settles the returned output receipt.
- `wgpu` includes `winit` and exposes `run_native`, `NativeApp` and `NativeOptions`.
  Implement `NativeApp::update` by returning the `DockspaceOutput` from
  `run_single_surface`. The runner owns the window loop, renderer and exact
  output settlement. `frame_finished` handles application commands after rendering.
- `accesskit` adds the system accessibility adapter. The runner connects it
  automatically; an existing event loop calls `init_accesskit` and forwards its
  user events through `on_accesskit_event`. Create the window hidden, initialize
  the adapter, then show it for the first time.

The [small editor](examples/winit.rs) shows this setup without custom event-loop
code. Configure window attributes through `NativeOptions::window`, and configure
fonts and theme on the supplied egui context. A root-window close request reaches
the application as viewport input; return `ViewportCommand::CancelClose` to keep
the window open for saving or confirmation. This runner owns one OS window and
reports an error for separate viewport or screenshot requests. Use a custom
host when those capabilities are needed.

The runner discards stale egui repaint messages and paces failed surface
acquisitions. Scheduled hidden-window updates still process visibility and close commands,
but run only when requested and at most once every 250 ms. Native input and
window restoration can wake the UI immediately.

The separate [`dockspace_winit`](../dockspace_winit) crate supplies mouse event,
coordinate and repaint helpers for other winit-based UI frameworks. It has no
egui or renderer dependency. GPUI uses its own platform event integration.

## Host choices

| Integration | Capabilities | Start here |
| --- | --- | --- |
| Your host owns `RawInput`, the complete egui run and final `FullOutput` | Full single-window docking | `Dockspace::run_single_surface_with_ui`; [official host](../../integration/egui-official-harness) |
| Ordinary `eframe::App::ui` callback | Rendering, selection, close, menus and keyboard resize; drag docking/focus confirmation unavailable | `Dockspace::show_single_surface`; [basic example](examples/basic.rs) |
| Fork-backed eframe with native windows | Experimental multiview and native lifecycle | [Native adapter](../egui_dockspace_native) and [platform support](../../docs/PLATFORM_SUPPORT.md) |

The complete-output host reports presentation only after the renderer presents
the corresponding output. A callback alone cannot supply that observation.

## Render your panes

Implement `PaneView` on your application registry. The layout stores `ItemId`
values; your registry keeps documents, editors and other pane state.

```rust
use egui_dockspace::{ItemId, PaneResponse, PaneView};

struct Documents {
    text: String,
}

impl PaneView for Documents {
    fn title(&self, _item: ItemId) -> Option<egui::WidgetText> {
        Some("Document".into())
    }

    fn ui(&mut self, _item: ItemId, ui: &mut egui::Ui) -> PaneResponse {
        PaneResponse::from(&ui.text_edit_multiline(&mut self.text))
    }
}
```

Return the ordinary content responses created in that call. Composite panes can
use `with_content` or `with_content_ids`. `title` returning `None` keeps an
unloaded pane's place in the layout. `Box<dyn PaneView>` is supported.

Callbacks are synchronous and may repeat across egui passes. Run I/O in your
application's background tasks, transfer completed data to the UI thread, and
request a repaint. The [background-save example](examples/background_save.rs)
uses an exact deferred-close token so save failure keeps the document open.

## More examples

Run these from the repository root. They use an ordinary eframe callback and
focus on application workflows; use the workbench above for drag docking.

| Example | Run command |
| --- | --- |
| [Basic pane rendering](examples/basic.rs) | `cargo run -p egui_dockspace --example basic --features example-app --locked` |
| [Background saving](examples/background_save.rs) | `cargo run -p egui_dockspace --example background_save --features example-app --locked` |
| [Layout presets and debounced autosave](examples/workspace_persistence.rs) | `cargo run -p egui_dockspace --example workspace_persistence --features example-app,serde --locked` |

[All examples](../../docs/EXAMPLES.md) · [Native OS-window example](../egui_dockspace_native/README.md)

## Styling

Override `PaneView::minimum_size`, `focus_target` or `focus_state` where needed.
`DockStyle` follows the current egui theme and exposes geometry, timing and color
overrides, including tab corners and outlines. Use `DockStyle::from_egui(ui)`
after changing fonts or density.

For denser editor chrome, use
`DockStyle::from_egui_with_density(ui, DockDensity::Compact)`. This reduces tab
padding, minimum width and group grips while retaining the current font size.
The `basic` example lets you compare compact and comfortable density.

Override `PaneView::tab_ui` for per-item backgrounds, icons, badges or buttons;
`Tab::paint`, `paint_background` and `paint_title` retain the default rendering
where useful. Return the final `TabVisuals` to coordinate built-in focus and
close cues. Reserve additional content width with `tab_content_width`.
`tab_bar_layout` and `tab_bar_ui` add fixed leading/trailing controls or a
scrolling `AfterTabs` slot. `tab_close_ui` and `drag_preview_ui` customize the
close icon and source ghost. `drop_preview_ui` receives the resolved target and
resulting area. Its default paints the result highlight and future tab shapes
for merging or reordering, including titles and committed order. Applications can add
their own colors, icons, outlines or an optional localized `paint_label` hint.
Selection, dragging, close controls and accessibility
stay in the adapter. Enable **Custom tabs** in the workbench for a
[small working example](examples/workbench/tabs.rs).

Close icon size and visibility are separate from its hit target. The group
grip can be permanent, shown on bar hover, or hidden with its space reclaimed.
Floating windows and popups accept independent frame appearance overrides.

Configure gesture bindings separately with `DockspaceBuilder::input` or
`Dockspace::set_input`:

```rust
use egui_dockspace::{DockInputConfig, DockingActivation};

let input = DockInputConfig {
    docking: DockingActivation::HoldToEnable(egui::Modifiers::ALT),
    cancel_drag: Some(egui::KeyboardShortcut::new(
        egui::Modifiers::NONE,
        egui::Key::Escape,
    )),
};
```

`Always` removes the modifier requirement; `HoldToDisable` is the default Shift
behavior. Additional held modifiers are allowed. Use `Modifiers::COMMAND` for
the platform's primary shortcut modifier, or set `cancel_drag: None` to let the
application own cancellation. Standard egui focus navigation and document
shortcuts remain with their existing input owners.

`DockPolicy` controls docking rules and close permissions. The
[integration guide](docs/guide.md) covers these options and the output contract.

With `serde`, use `save_document_json` and `from_document_json` to save and reopen
a layout. Reconnect pane data with `item_id_for_external_key`; business data
stays in your application. The persistence example loads `dockspace-layout.json`
on startup, saves before closing, and has an explicit **Save layout file** button.
If saving fails, the window stays open so the error remains visible.
Pass a different path after `--` to keep multiple files. Its named presets remain
in memory for the current process.

## Features

| Feature | Purpose |
| --- | --- |
| `serde` | Layout document save/restore; pane data and file storage remain application-owned |
| `example-app` | Build the packaged eframe examples |
| `winit` | Input and platform-output bridge for an existing winit event loop and renderer |
| `wgpu` | Complete-output single-window runner and renderer using official WGPU |
| `accesskit` | System accessibility delivery and action input for the winit integration |
| `native-render-support` | Internal seam for the unpublished native adapter |

## License

MIT OR Apache-2.0. See [third-party sources](../../THIRD_PARTY.md) for attribution.
