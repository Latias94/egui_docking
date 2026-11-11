# egui_dockspace

Docking layouts for egui, backed by the renderer-neutral `dockspace` core.

Add tabs, splitters, docking guides and floating groups. Keep pane state in your
application and render it through `PaneView`. The adapter supplies egui styling,
keyboard/AccessKit actions and application-controlled close requests.

**Pre-release:** use a path dependency from this checkout. Requires Rust 1.95
and egui 0.37. Native OS windows use a separate experimental adapter.

## Try the interactive editor

From the repository root:

```sh
cargo run --manifest-path integration/egui-official-harness/Cargo.toml --features demo --bin official-caller-owned-demo --locked
```

This official-egui/WGPU host supports drag docking, splitter resizing and
floating groups in one OS window. Its source shows the complete input-to-output
integration, including an application toolbar and renderer presentation results.

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

## Examples

- [Basic](examples/basic.rs): a small eframe integration, pane content and theme/font changes.
- [Background saving](examples/background_save.rs): worker completion, close approval and failure handling.
- [Workspace persistence](examples/workspace_persistence.rs): named presets, closed-pane inventory and layout locking.
- [All run commands](../../docs/EXAMPLES.md), including the full draggable and native editors.

## Styling

Override `PaneView::minimum_size`, `focus_target` or `focus_state` where needed.
`DockStyle` follows the current egui theme and exposes geometry, timing and color
overrides. Use `DockStyle::from_egui(ui)` after changing fonts or density.
`DockPolicy` controls docking rules and close permissions. The
[integration guide](docs/guide.md) covers these options and the output contract.

## Features

| Feature | Purpose |
| --- | --- |
| `serde` | Layout document save/restore; pane data and file storage remain application-owned |
| `example-app` | Build the packaged eframe examples |
| `native-render-support` | Internal seam for the unpublished native adapter |

## License

MIT OR Apache-2.0. See [third-party sources](../../THIRD_PARTY.md) for attribution.
