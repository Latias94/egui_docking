# egui_dockspace

`egui_dockspace` is the official-egui renderer for the renderer-neutral
`dockspace` core. Its default product facade provides interactive docking for
one logical egui surface, including tabs, close decisions, splitters, docking
guides, and contained floating presentations.

```rust
use egui_dockspace::{
    Dockspace, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
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
`Dockspace::run_single_surface` to retain one private candidate until the
terminal output and every output hook succeed:

```rust,ignore
let (output, response) = dockspace
    .run_single_surface(&context, raw_input, surface, panes)
    .expect("the Dockspace update publishes");
assert_eq!(
    response.presentation_settlement_capability(),
    egui_dockspace::DockspaceCapability::Supported,
);
let (full_output, receipt) = output.into_parts();
let result = renderer.present(full_output); // Host-owned Presented or Dropped result.
receipt.report(result);
```

Pane callbacks run once per actual egui pass. Repeated passes re-encode the
same accepted input candidate; only the intact terminal pass publishes. A failed
run leaves core state and producer watermarks unchanged and never automatically
replays application callbacks. Split the returned
`DockspaceOutput` at the renderer boundary with `into_parts()`. This transfers
`FullOutput` and an affine `DockspaceOutputReceipt` to the host. Report
`SurfacePresentationResult::Presented` only after that complete output is
presented, or `Dropped` if it is discarded. Receipt drop also retires its outputs.
Core accepts only the exact emission after a successful later frame commit;
output production alone grants no presentation or focus authority.

The owned run captures the original ordered input batch once, including an
explicit, representable `RawInput.time`. That batch observation time reaches
the core pointer ledger unchanged across egui reruns. Missing time remains
unknown; the adapter does not substitute egui's predicted frame time. This is
a batch clock, not individual OS event timestamps. Generic egui pointer events
retain Unknown device class because they can also be synthesized from touch.
With explicit batch time, the adapter also schedules the core's finite hover
continuation. A stationary active drag can select an inactive tab after the
default 150ms dwell without requesting pane-content focus. The wake submits an
explicit continuation with fresh receiver checks; it never fabricates pointer
motion. `DockStyle::hover_selection_delay` can customize or disable this behavior.

Shift temporarily disables magnetic acquisition. The adapter preserves ordered
`ModifiersChanged`, key and pointer-button modifier observations, including
changes within one raw packet. A modifier-only frame can clear an existing
magnetic preview without reporting mouse motion. Releasing Shift leaves that
preview absent until a subsequent real Move or Release resolves a fresh target.
Explicit docking guides and tab-gap targets remain available, and contained
content keeps its current drag position. This intent is independent from the
arrangement lock and cannot alter an ordinary release which already committed.

The same finite continuation drives stationary overflow-edge scrolling during
a local drag. `DockStyle::tab_strip_edge_scroll_band` defaults to 24 points and
`tab_strip_edge_scroll_interval` to `Some(50ms)`; zero band or `None` disables it.
Each step waits for the exact newly scrolled output to be presented before
scheduling another. The renderer receipt wakes an otherwise idle host to accept
that result, without polling or inventing motion. Close/strip controls and
blockers stop automatic scrolling; explicit scroll and menu selection take
ownership through the existing core paths.

Explicit `Event::Touch` contacts keep independent provider pointer identities
and Known-Touch metadata. A packet with explicit or live touch input does not
also submit its potentially synthesized generic pointer events to docking.
The ordinary egui events still reach pane widgets, including content scrolling.
On dock chrome, a short tap activates on contact end, movement beyond 8 points
before arming yields to content, and a 350ms stationary continuation arms docking.
Armed motion uses the ordinary drag/resize path; stationary armed release opens
the existing root presentation menu without selecting or focusing the tab.
`DockStyle::touch_long_press_delay` and `touch_press_slop_distance` customize the
profile. Read `DockspaceResponse::touch_events()` for ordered semantic events,
including context-menu requests. Missing batch time disables timed arming while
preserving short taps. Explicit Touch uses the core output's expanded splitter,
junction and contained resize targets; `DockStyle::touch_resize_hit_extent`
defaults to 44 points before surface/protected-chrome clipping. Mouse, pen and
unknown-device hit geometry retain their existing profile. Ordinary interactive
widgets and disabled UI still block docking. Native touch/stationary capture
remains separate work.

The adapter validates the complete ordered surface span after all output hooks,
including ordinary geometry, previews, clipping and transforms. Pane drawing must
be synchronous and append-only on the supplied Ui layer; another-layer pane write or
an earlier-shape mutation conservatively disables settlement. Adapter-owned popup
chrome may append to registered foreground layers, with each owned suffix checked
by the same terminal proof. Honor the supplied
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
full draggable editor path. Each accepted callback publishes independently;
late hooks or an enclosing run cannot roll that publication back. The composed
`egui_dockspace::run_ui` wrapper offers output settlement for callback integration
with the same limitation. UI-generated actions are bounded and enter the next
update before its encoding phase.

An ordinary logical release commits once with accepted input, including when
output capacity defers rendering. Later dropped output does not cancel it.
Native resource transfers keep their separate first-live ownership barriers.

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
`Dockspace::reset_layout(default_layout)` applies an ordinary application-owned
layout through the same core transaction without enabling serde. The example's
Default button uses this path; named user snapshots use document restore.

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
