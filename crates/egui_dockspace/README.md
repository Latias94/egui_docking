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
cargo run -p egui_dockspace --example workbench --features example-wgpu --locked
```

This official-egui/WGPU host supports drag docking, splitter resizing and
floating groups in one OS window. Start with
[`workbench/editor.rs`](examples/workbench/editor.rs) for layout, pane rendering,
styling and close decisions. The [window host](examples/workbench.rs) and
[renderer](examples/workbench/renderer.rs) show complete input and presentation.

Drag tabs onto a pane to merge or split it. To float a tab, hold **Shift** while
dragging and release inside the window. Shift temporarily disables docking. Drag a
floating group's dotted title grip onto a docking guide to dock the group again.

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
| [Layout persistence](examples/workspace_persistence.rs) | `cargo run -p egui_dockspace --example workspace_persistence --features example-app,serde --locked` |

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
remaining decorations. Selection, dragging, close controls and accessibility
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

## Features

| Feature | Purpose |
| --- | --- |
| `serde` | Layout document save/restore; pane data and file storage remain application-owned |
| `example-app` | Build the packaged eframe examples |
| `example-wgpu` | Build the complete-output workbench using official winit/WGPU |
| `native-render-support` | Internal seam for the unpublished native adapter |

## License

MIT OR Apache-2.0. See [third-party sources](../../THIRD_PARTY.md) for attribution.
