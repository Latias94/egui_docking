# egui integration guide

[Crate overview](../README.md) · [Examples](../../../docs/EXAMPLES.md)


- [Pane rendering](#pane-rendering) and background work
- [Complete-output hosts](#complete-output-hosts) and [callback hosts](#callback-hosts)
- [Pointer input](#pointer-input) and [output contract](#output-contract)
- [Close decisions](#closing-content-and-dock-back)
- [Committed lifecycle events](#committed-lifecycle-events)
- [Feedback and focus](#feedback-and-focus)
- [Styling](#styling) and [features](#features)

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

## Pane rendering

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

`PaneView` callbacks are synchronous and can run again in a later egui pass.
Keep I/O and background computation in application-owned tasks. Transfer finished
data to the UI thread, update pane state, and request a repaint. `Box<T>` forwards
all pane callbacks, including custom focus observations; ordinary egui hosts can
use borrowed or non-`Send` providers. The native adapter requires owned `Send`
providers because deferred viewport callbacks retain them.

The [background save example](../examples/background_save.rs) defers a tab close,
saves an owned text snapshot on a worker, and submits the original continuation
token on the UI thread. Success allows the close; failure or a stopped worker
vetoes it. Stale completion cannot close a replacement layout. The example pauses
editing while saving; applications that keep editing must also check their own
document revision before allowing closure. Run it with
`cargo run -p egui_dockspace --example background_save --features example-app`.
It writes a new copy in the system temporary directory and uses no async runtime.

## Complete-output hosts

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

An owned run reserves output capacity before invoking egui or application UI.
If earlier renderer outputs fill that budget, it returns
`DockspaceErrorKind::OutputBackpressure` with the original, unconsumed `RawInput`:

```rust,ignore
match dockspace.run_single_surface(&context, raw_input, surface, panes) {
    Ok((output, response)) => {
        // Render this exact output and report its receipt as usual.
    }
    Err(error) => {
        pending_input = Some(error.into_deferred_input()?);
        // Wait for earlier renderer receipts to settle. Retry this batch before
        // taking later input from the host queue; do not spin or replay callbacks.
    }
}
```

Deferral preserves event order, timestamps, viewport metadata and focused IME
state without running a partial egui frame. Other errors do not return replayable
input. The ordinary `run_native` runner settles each output immediately, so it
does not normally encounter this condition. Lower-level hosts using `run_ui`
with `show_single_surface` must bound their own outstanding outputs before
consuming ordinary pane input; a callback cannot undo input already taken by
the surrounding egui run.

## Pointer input

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

`DockInputConfig` controls gesture bindings through `DockspaceBuilder::input`
and `Dockspace::set_input`. `DockingActivation` supports always-on docking,
hold-to-enable and hold-to-disable modifier chords. `cancel_drag` accepts an
optional egui `KeyboardShortcut`. Required modifiers use subset matching, so
extra modifiers do not accidentally restore docking or prevent cancellation.
`Modifiers::COMMAND` follows egui's platform mapping. Runtime changes recompute
the last observed modifiers at the next input boundary, without fabricated
motion. Explicit touch contacts use the same settings.

By default, Shift temporarily disables docking acquisition. The adapter preserves ordered
`ModifiersChanged`, key and pointer-button modifier observations, including
changes within one raw packet. A modifier-only frame can clear an existing
docking preview without reporting mouse motion. Releasing Shift leaves that
preview absent until a subsequent real Move or Release resolves a fresh target.
Explicit docking guides and tab-gap targets are also suspended, and contained
content keeps its current drag position. This intent is independent from the
arrangement lock and cannot alter an ordinary release which already committed.

Docking selects a destination from the currently presented panes. Its highlight
shows the candidate result after removing the payload from its source and
inserting it at that destination. Moving the last tab out of a pane can expand
the remaining panes, so the highlight can differ from the destination's current
bounds. A tab-gap target shows an insertion line at the resulting tab position.
Use `DockVisualOverrides::drop_fill` and `drop_border_color` for these cues.
`PaneView::drop_preview_ui` receives the core-resolved target, current bounds,
selected item, insertion neighbors and result rectangle. Applications can add
colors, icons or other destination feedback without another hit test. The
default draws the result highlight and projected target tabs, without extra
annotations. The future tabs use committed order, selection, scrolling and
clipping; split targets and hidden bars do not invent a tab preview. Optional
`paint_target` and `paint_label(ui, text)` helpers draw an outline or an
application-provided accessible hint. `drag_preview_ui` decorates the moving
source ghost separately.

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

## Output contract

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
cargo run -p egui_dockspace --example workbench --features wgpu --locked
```

Enable `wgpu` to reuse the public single-window `winit::run_native` runner, or
enable `winit` for `WinitIntegration` with an existing event loop and renderer.
Both are optional. The [small editor](../examples/winit.rs) demonstrates the
runner; the [crate README](../README.md#use-winit) covers setup and the optional
system accessibility adapter.

## Callback hosts

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
tokens are inert. The packaged examples consume these decisions and keep pane
data owned by the application. The persistence example offers a closed-pane
inventory and reopens stable items through `open_item_current`; it also applies
application-owned named layout documents.

Use the distinct `Dock Back` menu item or `dock_root_back_current` to return
floating content to its docking destination while keeping it open. Closing and
Dock Back share stable item/root identities, but execute different commands.

For a complete tabs leaf, `capture_tabs_group(DockAnchor::Item(item))` returns a
`PreparedTabsGroup`. Pass it by value to `dock_tabs_current` or
`float_tabs_current`; the group moves in one transaction with its order and
selected item preserved. A captured group belongs to its original session and
workspace version. Recapture after layout changes rather than replaying an old
anchor against a different group.

## Committed lifecycle events

Read `result.mutation().events()` after a successful docking call to react to
committed selection/MRU changes, tab reorder, opening, placement changes, or
approved closes. The same `DockspaceMutation::events()` API is available on
ordinary surface responses and programmatic action/close results. Layout reset
and document restore return their events directly on the mutation. Headless
hosts receive the same product events through `HostFrameReport::events()`.

```rust,ignore
for event in response.mutation().events() {
    match event.kind() {
        DockspaceEventKind::ItemOpened { item, .. } => app.pane_opened(*item),
        DockspaceEventKind::Closed(outcome) => app.content_closed(outcome),
        _ => {}
    }
}
```

These are read-only notifications from one successful core publication, not
before/after drawing hooks. The owned `run_single_surface` call returns one
batch after its terminal pass; discarded encoding passes do not publish it.
Callback hosts using `show_single_surface` already commit inside each successful
callback. Collect their notifications across callbacks rather than keeping
only the final pass's response, then process them outside the outer egui run.
A later egui output discard does not undo those earlier core publications.
Repeated passes do not replay input. Reading or cloning a result preserves each
opaque `event.id()`; applications that retain notifications can deduplicate.
`event.version()` records the version after that event's mutation, which can
precede the report's final version when a frame contains several changes.

No-op, rejected and stale actions do not emit completion events. A close
request, reused plan, deferred decision or veto is not a successful close.
Presentation-gated native transfers emit placement changes only after content
ownership commits; rendering a prospective target is insufficient. Layout
replacement emits one reset/restore notification and suppresses earlier
changes superseded inside that same atomic commit. `SelectionCommitted`
includes selection or MRU changes and does not assert keyboard or window focus.

Keep pane and tab drawing callbacks synchronous. Use `tab_ui` for custom tab
painting and `tab_bar_ui` with `Leading`, `Trailing` or `AfterTabs` for controls
around tabs; queue requested edits and handle lifecycle notifications outside
those potentially repeated callbacks.

## Feedback And Focus

Programmatic actions distinguish publication failure from product rejection.
`Ok(result)` means the atomic update committed; check its status before reporting
that a menu command succeeded:

```rust,ignore
let result = dockspace.open_item_current(item, placement)?;
match result.status() {
    egui_dockspace::DockspaceActionStatus::Applied(_) => show_opened(item),
    status => show_action_unavailable(status),
}
```

`Rejected` carries a policy or topology reason, and `Stale` identifies a prepared
action whose revision is no longer current. `Err` means the action could not be
submitted or its host update could not publish. Custom menus and toolbars should
consume the returned status and feedback at that call boundary.

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

For an editable pane, assign a stable `.id(...)` to its primary editor and return
the same ID from `PaneView::focus_target`; the small
[winit editor](../examples/winit.rs) demonstrates this pairing. An already focused
target keeps its ongoing IME composition while its presentation acknowledgement
is pending. Each request applies framework focus once; a later application click
or focus transfer takes precedence while its exact renderer acknowledgement is
still pending. Stable unfocused observations remain available without requesting
continuous repaints. A new explicit selection can request pane focus again.
Applications continue to own document text and composition behavior.

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
style.tab_corner_radius = egui::CornerRadius::same(6);
style.visuals.tab_active_fill = Some(egui::Color32::from_rgb(40, 100, 120));
style.visuals.tab_active_stroke = Some(egui::Stroke::new(1.0, egui::Color32::WHITE));
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

For compact editor tabs, derive all related metrics together:

```rust
let mut style = egui_dockspace::DockStyle::from_egui_with_density(
    ui,
    egui_dockspace::DockDensity::Compact,
);
style.visuals.tab_active_fill = Some(egui::Color32::from_rgb(40, 100, 120));
dockspace.set_style(style)?;
```

`Compact` reduces tab padding, the minimum width of short titles and the group
grip width. Text stays at the current egui font size; pane content, UI zoom and
touch resize targets are unchanged. `Comfortable` follows egui's normal button
spacing and interaction height, as does `DockStyle::from_egui`. After a font
change, derive the style again with the selected density and reapply application
overrides. All returned fields remain editable for application-specific sizing.
The `basic` example has density and font controls. The workbench uses explicit
24-point tabs, six-point padding and a smaller group grip. Its selected tab and
pane share egui's panel fill, and inactive tabs blend into the tab bar. Enable
**Custom tabs** to add icons, rounded corners, an accent outline and fixed controls.

Tab titles elide within their allocated text area. The full title remains
available on hover, and the open-tab menu uses its own available width instead
of reusing the shortened tab label. That menu inherits egui's popup frame,
shadow, row spacing, and selected-item colors. A global `override_text_color`
also applies to tab text unless a corresponding docking text override is set.

`PaneView::title` can return `RichText` or a multicolor `LayoutJob`. Explicit
span colors survive in tabs, overflow rows and drag previews. The corresponding
`tab_text_color` / `tab_active_text_color` override takes precedence, followed
by the local egui `override_text_color`, then explicit span colors. Uncolored
spans follow the current interaction state. Disabled UI retains egui's standard
tint; no per-pane color configuration is needed.

### Custom tabs and tab-bar controls

All tab callbacks are optional methods on `PaneView`. Keep the existing `title`
method: it supplies measurement, hover text, accessibility names, overflow rows
and drag-preview text even when the visible tab is custom.

For compact single-pane groups, declare
`TabBarPolicy::new(TabBarVisibility::AutoHideSingle, TabBarInteraction::Enabled)`
through `DockPolicy::set_tab_bar`. Core hides a one-member bar and keeps a small
accessible corner button to reveal it. The revealed bar stays open until its
membership changes; adding a second tab shows the bar automatically. Empty
central regions retain their ordinary presentation. The same rule determines
layout minima and future docking previews. Explicit `Hidden` omits the bar and
its recovery button entirely.

- `tab_content_width(item, ui, title)` returns intrinsic content width before
  docking padding and the close button. The default is the shaped title width.
  Add space here for custom icons, badges or buttons; measurements must be
  finite, non-negative and independent of an earlier paint pass.
- `tab_ui(ui, tab)` receives the original response, selected/enabled state,
  unclipped allocation, visible clip, content rect and resolved `TabVisuals`.
  Call `tab.paint(ui)` for defaults, or paint the background and title separately.
  `tab.paint_title(ui, rect)` elides and clips the title to a content subrect.
  Return the final `TabVisuals`: built-in focus cues inherit its corners and
  strokes; `close_color` can coordinate the close icon with a custom fill.
  Its default retains egui's independent interaction colors. `tab.pane_focused` reports
  observed content focus separately from tab selection and keyboard focus.
- `tab_close_ui(ui, close)` paints the close icon. Its response, hit target,
  keyboard actions, accessibility and close request remain adapter-owned.
- `tab_bar_layout(bar_id, ui)` reserves fixed leading/trailing widths outside
  the scrollable strip, plus an optional `after_tabs_width` inside it.
  `tab_bar_ui(ui, bar, slot)` draws into each visible allocation.
  `bar.items()` includes every group member in tab order.
- `drag_preview_ui(ui, preview)` paints the non-interactive drag ghost. Reuse
  its background/title helpers to add the same icons or badges as the tabs.
  The callback is skipped when core projects the actual floating source.
- `drop_preview_ui(ui, preview)` paints the result highlight and future tabs. Its semantic target
  describes the currently presented layout; its result rectangle describes the
  pending layout. `DropPreview::tabs` exposes paint-only members; each
  `DropPreviewTab` has background/title helpers for custom colors or icons.
  `paint_target` and `paint_label(ui, text)` are optional helpers
  for applications that want additional annotations.

For example, customize one item's background without copying the renderer:

```rust
fn tab_ui(
    &mut self,
    ui: &mut egui::Ui,
    tab: &egui_dockspace::Tab<'_>,
) -> egui_dockspace::TabVisuals {
    let mut appearance = tab.clone();
    if self.needs_attention(tab.item) {
        appearance.visuals.fill = egui::Color32::from_rgb(120, 75, 25);
    }
    appearance.paint(ui)
}
```

Application widgets belong inside `tab.content_rect`; keep the built-in close
area free. They retain ordinary egui input and block docking over themselves,
without any `PaneResponse` declaration. The adapter continues to own selection,
close requests, dragging, keyboard navigation, focus cues and accessibility.
Missing panes use the built-in fallback, and dragged source gaps skip custom
tab/slot callbacks. Disabled groups pass disabled Uis to application callbacks.

`Leading` and `Trailing` stay fixed while tabs scroll. Core prioritizes the
leading slot and may shrink or omit either fixed slot in a narrow group to
preserve an operable tab. `AfterTabs` follows the last tab and contributes to
scrolling width, like a browser's add button. It keeps its requested width;
the supplied clip determines its visible and interactive portion. Use
the supplied Ui's actual width and clip rather than the requested width. The
callbacks render synchronously into the supplied Ui; queue workspace edits in
application state and apply them after the docking call returns.
`Box<dyn PaneView>` forwards every callback. Native windows share
the same implementation and `PaneView` interface.

`DockStyle::tab_close_visibility` supports always-visible, selected-or-hovered,
and hover-only icons. Keyboard focus reveals the icon in every mode. The icon
size (`tab_close_icon_size`) is independent of the hit target (`tab_close_size`),
so reducing visual weight does not shrink the clickable area or move the title.
Use the close painting callback for custom shapes or per-item colors.

`tab_group_grip_visibility` supports `Always`, `Hovered` (when the bar is
hovered), and `Hidden`. Hidden grips reclaim their layout space. Unoccupied
space after tabs and application chrome still drags the group; a full or
overflowing bar may have no such space, so retain the grip when that gesture
must always be discoverable.

Floating windows and popup menus have independent `DockFrameVisuals` overrides:

```rust
style.visuals.floating.corner_radius = Some(egui::CornerRadius::same(8));
style.visuals.floating.fill = Some(egui::Color32::from_gray(35));
style.visuals.popup.corner_radius = Some(egui::CornerRadius::same(4));
```

Each recipe supports fill, border color, corners and shadow; unspecified values
follow the local egui theme. Floating title bars also expose active/inactive
fills and text color. Frame overrides leave core-owned margins and hit regions
under the existing geometry settings.

The workbench's **Custom tabs** toggle demonstrates title icons, rounded
backgrounds, coordinated drag previews, close-button hover feedback, group
counts, a fixed **Tint** button and a scrolling help button. Its
[tab customization module](../examples/workbench/tabs.rs) is separate
from the host and pane-content code.

A selected tab gains a two-point underline when its pane owns observed content
focus. This is distinct from selection and the outline around a keyboard-focused
tab. The cue uses egui's selection stroke color; override
`style.visuals.pane_focus_stroke`, or set it to `Stroke::NONE` to hide it.
The stroke width controls underline thickness. `tab_focus_stroke` independently
controls the keyboard-focus outline and follows each tab's final corner radius.
Expose `PaneView::focus_target` (or an authoritative composite `focus_state`) so
the existing host-focus sampling can identify the pane. Selection, a pending
focus request, and the frontmost floating window alone do not produce the cue.
Unfocused windows and disabled tab strips do not show it.
The experimental native host refreshes exact native focus after route enrollment
and ingress recovery. A pending request does not establish observed focus.

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
Appearance menu demonstrates font and density changes; egui still owns the font resources.

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
[native overview](../../egui_dockspace_native/README.md).

Use `NativeCommand::SetPolicy { policy: Box::new(policy) }` to replace the complete
runtime docking policy, including per-pane move/close restrictions and docking
compatibility rules. Retain an application-owned policy value when later edits
should preserve previous rules. The command uses the same bounded slot and
configuration boundary as the arrangement lock: admission does not publish the
policy, and a dropped frame cannot partially apply it. Consume
`NativeActionStatus::PolicyChanged` before submitting the next command.
`SetArrangementLocked` remains the convenience command for changing only that
flag while preserving every other committed rule.

Native `with_view` returns the callback result directly. The committed layout
remains readable before host attachment and after fatal shutdown. View/pane
callbacks hold the runtime lock, so perform other runtime queries and requests
outside them.

## Features

Run the bounded editor layout example with:

```sh
cargo run -p egui_dockspace --example workspace_persistence --features example-app,serde --locked -- my-layout.json
```

It has eight application-owned panes, text/scroll content and a small drawn
scene. Its closed inventory, immutable default and at most eight user presets
remain in application memory. The current layout loads from the supplied JSON
file at startup and saves before closing or through **Save layout file**.
A failed close-time save keeps the window open with a visible error and an
explicit **Exit without saving** option. An unreadable
or invalid existing document is preserved until an explicit save. File writes
use a temporary file in the destination directory and replace the document
after successful encoding and writing. Layout locking uses `DockPolicy` authority;
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
[platform support matrix](../../../docs/PLATFORM_SUPPORT.md); those claims do not
extend the portable adapter's own host contract.
