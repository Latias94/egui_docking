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
Its `ui` callback returns a `PaneResponse` declaring ordinary content from that
same call. A single content control needs no per-pane widget-ID bookkeeping:

```rust,ignore
fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) -> PaneResponse {
    let document = self.documents.get_mut(&item).expect("registered document");
    let editor = ui.text_edit_multiline(&mut document.text);
    PaneResponse::from(&editor)
}
```

Use `with_content` or `with_content_ids` for composite content. Only declare
ordinary controls created during that callback; popups and foreign overlays
remain docking blockers. Returning `PaneResponse::default()` declares no
content. Focus targets remain separate application observations.

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

For application menus, toolbars, sidebars or status panels, use the same atomic
path with a composition callback:

```rust,ignore
let mut restore = false;
let (output, response) = dockspace.run_single_surface_with_ui(
    &context, raw_input, surface, panes, |ui| {
        egui::Panel::top("toolbar").show(ui, |ui| {
            restore |= ui.button("Restore layout").clicked();
        });
    },
)?;
// Docking occupies the remaining Ui. Apply application commands after success.
if restore {
    dockspace.reset_layout(default_layout())?;
}
```

The composition callback runs once per actual egui pass, like pane UI. Repeated
passes may invoke it again; application-side mutations are not rolled back by
a rejected final output. Collect command intents and apply them after success.
The original `run_single_surface` delegates to this same driver with no panels.

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

Geometry remains explicit. To match a customized font or control density, call
`DockStyle::from_egui(ui)` during a UI pass, then pass the result to the builder
or `Dockspace::set_style`. This reads the actual font height, button padding,
interaction height, and window border width. Call it again after an application
font/density change; whole-UI zoom already scales logical points and needs no
extra geometry multiplication. Reapply any application geometry/color overrides
to the derived style before submitting it. The `basic` example demonstrates
theme and font changes. `DockStyle::default()` retains its fixed geometry for
applications which configure these metrics directly.

Tab titles elide within their allocated text area. The full title remains
available on hover, and the open-tab menu uses its own available width instead
of reusing the shortened tab label. That menu inherits egui's popup frame,
shadow, row spacing, and selected-item colors. A global `override_text_color`
also applies to tab text unless a corresponding docking text override is set.

A selected tab gains a two-point underline when its pane owns observed content
focus. This is distinct from selection and the outline around a keyboard-focused
tab. The cue uses egui's selection stroke color; override
`style.visuals.pane_focus_color`, or set it to `Color32::TRANSPARENT` to hide it.
Expose `PaneView::focus_target` (or an authoritative composite `focus_state`) so
the existing host-focus sampling can identify the pane. Selection, a pending
focus request, and the frontmost floating window alone do not produce the cue.
Unfocused windows and disabled tab strips do not show it.
The experimental native host refreshes exact native focus after route enrollment
and ingress recovery; the [focus verification](../../docs/audits/2026-11-09-native-startup-focus-refresh.md)
records the cold-start and background-launch behavior.

For the experimental native host, create the session with the same
`style.presentation_config()?` before passing both objects to `NativeDockspaceApp`.
Native construction rejects mismatched geometry/timing instead of displaying a
style whose core hit regions use different metrics. Colors and ghost offset stay
adapter-local; custom core-only configuration remains authoritative.
After a runtime font or density change, queue
`native_app.request_style(DockStyle::from_egui(ui))?`, with application overrides
reapplied. This shares the existing application command slot. Consume its
`NativeActionStatus::StyleChanged` result before waiting for fresh presentation
or quiescence. Core geometry and adapter styling publish together after the
final root pass commits. The first and repeated encoding passes already use the
candidate style and its matching new core geometry; the public style remains
unchanged until publication. Core-only configuration is preserved. Geometry changes
cancel active gestures through core; visual-only and unchanged styles leave
those gestures intact. Every live window is repainted. The native example's
Appearance menu demonstrates font changes; egui still owns the font resources.

Managed child OS windows derive descriptive titles from each tab group's
selected pane, including split and contained groups. Pane selection and renamed
titles update the caption without changing the native window identity. Missing
content uses stable item/surface fallback names. The application retains its
own Root title and may still issue explicit viewport title commands.

Native menus submit `egui_dockspace_native::NativeCommand` values through
`NativeDockspaceApp::request_command`. Item docking/floating and contained
raise/bring-into-view use the same bounded command slot as selection, reopening,
close decisions and native-window promotion. Read `take_action_status` on a
later update and consume its terminal result before submitting another command
or waiting for quiescence. Acceptance into the queue is separate from core
success and actual surface presentation. Layout/document replacement keeps its
dedicated preparation methods; see the
[migration guide](../../docs/ENGINE_FIRST_MIGRATION.md#egui-integration).

Native `with_view` returns the callback result directly. The committed layout
remains readable before host attachment and after fatal shutdown. View/pane
callbacks hold the runtime lock, so perform other runtime queries and requests
outside them.

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
fork-backed `egui_dockspace_native` crate remains unpublished. Its published
fork revisions and finite native/X11 qualifications are described in the
[platform support matrix](../../docs/PLATFORM_SUPPORT.md); those claims do not
extend the portable adapter's own host contract.
