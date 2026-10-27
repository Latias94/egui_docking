# egui_dockspace

`egui_dockspace` is the official-egui renderer for the renderer-neutral
`dockspace` core. Its default product facade provides interactive docking for
one logical egui surface, including tabs, close decisions, splitters, docking
guides, and contained floating presentations.

```rust
use egui_dockspace::{
    run_ui, Dockspace, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};

let item = ItemId::new(1);
let surface = SurfaceId::new(1);
let root = RootId::new(1);
let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
    surface,
    DockspaceRootLayout::new(root, DockspaceNode::central_tabs([item])),
)])?;
let mut dockspace = Dockspace::builder("my-dockspace", layout).build()?;

assert!(dockspace.view().item(item).is_some());
# Ok::<(), Box<dyn std::error::Error>>(())
```

Provide a `PaneView` implementation to measure and paint application panes.
When the integration owns the complete official-egui run, use
`egui_dockspace::run_ui` so Dockspace can terminally settle previews after every
output hook succeeds:

```rust,ignore
let output = run_ui(&context, raw_input, |ui| {
    let response = dockspace
        .show_single_surface(surface, ui, panes)
        .expect("the Dockspace frame advances");
    assert_eq!(
        response.presentation_settlement_capability(),
        egui_dockspace::DockspaceCapability::Supported,
    );
});
```

The callback can run more than once when egui discards a pass. Dockspace keeps
the run identity private, admits only the terminal output after `run_ui`
returns, and abandons the active evidence on unwind. The current private receipt
checks a preview compound in terminal output; it does not prove complete surface
acceptance, GPU submission, or physical window presentation. Replacing the remaining
preview-only acknowledgement path with the core's exact output contract is still
U2 work. No caller-supplied frame count can strengthen this evidence.

The repository includes a real-window official-egui host that exercises this
supported tier directly:

```text
cargo run --manifest-path integration/egui-official-harness/Cargo.toml --features demo --bin official-caller-owned-demo --locked -j1
```

An unmodified `eframe::App::ui` callback can call
`Dockspace::show_single_surface` directly, but eframe does not expose the
surrounding `RawInput` and terminal `FullOutput`. That path truthfully reports
`PresentationSettlementRequired`: rendering, selection, close, menus, and
discrete keyboard/AccessKit actions remain available, while pointer gestures
that require preview or focus settlement are disabled. See the packaged
`basic` example for this boundary and the fork-backed native example for the
full draggable editor path.

For a non-root caller-owned viewport, terminal pane-focus confirmation uses the
viewport's final AccessKit tree. Enable AccessKit and ensure the `focus_target`
identity belongs to the pane's emitted accessibility node; otherwise Dockspace
settles that request as unavailable rather than guessing from the root viewport.

## Styling

The application owns the surrounding egui `Panel`, `Frame`, and margins.
Dockspace consumes the available rectangle of the `Ui` it is given and does not
insert another panel around the docking surface.

`DockStyle::default()` resolves its colors from that `Ui`'s current
`egui::Visuals` on every pass. Dark/light theme changes therefore update tabs,
panes, splitters, menus, guides, and previews without replacing core layout
state. Applications can override only the tokens that belong to their visual
language:

```rust
use egui_dockspace::{DockStyle, Dockspace};

let mut style = DockStyle::default();
style.visuals.tab_active_fill = Some(egui::Color32::from_rgb(40, 100, 120));
let dockspace = Dockspace::builder("my-dockspace", layout)
    .style(style)
    .build()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Features

- `serde`: product document save and restore.
- `example-app`: builds the eframe examples.
- `native-render-support`: internal rendering seam for the unpublished native
  adapter; it is not a public multiview runtime.

Native multi-viewport support is intentionally not claimed by this crate. The
fork-backed `egui_dockspace_native` workspace has one bounded real-window
lifecycle smoke, but remains unpublished until its fork revision is remotely
reproducible and its CI, platform capability, and physical cross-window input
gates pass.
