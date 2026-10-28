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
`egui_dockspace::run_ui` to retain complete output ownership after every
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
let (full_output, receipt) = output.into_parts();
let result = renderer.present(full_output); // Host-owned Presented or Dropped result.
receipt.report(result);
```

The callback can run more than once when egui discards a pass. Split the returned
`DockspaceOutput` at the renderer boundary with `into_parts()`. This transfers
`FullOutput` and an affine `DockspaceOutputReceipt` to the host. Report
`SurfacePresentationResult::Presented` only after that complete output is
presented, or `Dropped` if it is discarded. Receipt drop also retires its outputs.
Core accepts only the exact emission after a successful later frame commit;
output production alone grants no presentation or focus authority.

The adapter validates the complete ordered surface span after all output hooks,
including ordinary geometry, previews, clipping and transforms. Pane drawing must
be synchronous and append-only on the supplied Ui layer; another-layer write or
an earlier-shape mutation conservatively disables settlement. Honor the supplied
opacity and source-gap suppression. Private markers are removed while user
shapes and callbacks remain intact. Shape comparison cannot inspect callback
internals. After `into_parts()`, preserving the exact complete output is the
trusted renderer owner's responsibility. No frame count is presentation proof.

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

## Closing Content And Dock Back

The contained-window close button and `Close Group` menu item request approval
to close the whole root. `DockspaceResponse::close_request_events()` returns the
plan; the application decides each pending item through `resolve_close` and, for
deferred decisions, `resolve_deferred_close`. Until all required decisions allow
closure, the layout keeps every item. Veto preserves the entire group. Replayed
tokens are inert. Both packaged examples consume these decisions and keep pane
data owned by the application. The persistence example offers a closed-pane
inventory and reopens stable items through `open_item_current`; it also applies
application-owned named layout documents.

Use the distinct `Dock Back` menu item or `dock_root_back_current` to return
floating content to its docking destination while keeping it open. Closing and
Dock Back share stable item/root identities, but execute different commands.

## Feedback And Focus

Read `DockspaceResponse::manipulation_feedback()` for ordered pending and terminal
statuses from a shown surface. Direct commands and close decisions return their
own feedback through `result.mutation().manipulation_feedback()`; policy changes
and document restore return the `DockspaceMutation` directly. Consume that result
at the call boundary. The next shown frame does not replay it. Both paths retain
the core's original manipulation identity and per-publication deduplication ID.
Motion alone is quiet, and reading feedback never requests focus.

Explicit selection requests pane focus even when the item was already selected.
Opening new content selects it and requests the same pane focus intent;
opening an existing item preserves its location, selection and focus.
Dock and Float commands focus the moved visible pane; closing a focused tab uses
its next visual sibling, or the previous sibling at the end. A focused group with
no surviving observed destination focuses the remaining workspace. These requests
still require the adapter's exact output and focus-observation boundary. The
ordinary callback-only eframe examples continue to report that boundary as
unavailable rather than claiming focus settlement.

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

Run the bounded editor layout example with:

```sh
cargo run -p egui_dockspace --example workspace_persistence --features example-app,serde
```

It has eight application-owned panes, text/scroll content and a small drawn
scene. Its closed inventory, immutable default and at most eight user presets
remain in application memory. Layout locking uses `DockPolicy` authority;
reset/apply/reopen controls require an explicit unlock, while normal pane use
continues. Saved layouts contain docking state and stable keys, not pane data.
The example resolves current presentation identities after a preset restore.
Unmodified eframe still reports presented-output drag docking unavailable.

Document key recognition and pane availability are separate application facts.
Recognize a valid persisted key even when its plugin or resource cannot currently
load. Returning `None` from `PaneView::title` renders a missing-pane placeholder
and reports the stable item through `DockspaceResponse::missing_panes()`; layout
ownership is retained. A missing focus target terminates that exact request as
unavailable, without claiming focus. When the registry entry becomes available,
the same item renders in place. An ordinary selection can issue a new focus
request; the completed unavailable request does not replay. Rejecting a document
key instead rejects the complete identity association. Removing unavailable
content is an application decision through the ordinary close workflow.

- `serde`: product document save and restore.
- `example-app`: builds the eframe examples.
- `native-render-support`: internal rendering seam for the unpublished native
  adapter; it is not a public multiview runtime.

Native multi-viewport support is intentionally not claimed by this crate. The
fork-backed `egui_dockspace_native` workspace has one bounded real-window
lifecycle smoke, but remains unpublished until its fork revision is remotely
reproducible and its CI, platform capability, and physical cross-window input
gates pass.
