# egui_dockspace_native

Experimental native-window hosting for `egui_dockspace`.

This unpublished adapter connects Dockspace to pinned eframe/winit forks. It
adds native window creation and recovery, per-window output reporting and
multiview input integration. The core remains the sole docking-state owner;
your application owns pane content.

Pane rearrangements in this adapter are immediate. The official egui adapter's
configurable layout motion is not yet connected to this native driver. Native
window movement, including the solar-system example, follows its own application
clock and the desktop compositor.

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

For a small multiview demonstration with a central Sun and three orbiting OS
windows:

```sh
cargo run --manifest-path integration/egui-fork-workspace/Cargo.toml -p egui_dockspace_native --example solar_system --locked
```

Pause the orbit to drag and dock windows by hand. Closing a planet window
recovers its pane; **Launch returned planets** creates new windows for panes
back in the main window. A combined multi-pane window stops orbiting so its
panes do not issue conflicting position requests. The example needs a backend
with native creation and desktop placement support. Add `-- --smoke` for a
finite real-window create/move/Dock-Back/recreate check. The smoke uses the
public programmatic recovery command; it does not simulate an OS close-button event.
One application clock requests movement at 60 Hz; child windows report their
actual geometry without starting independent animation timers. Use `--release`
when assessing motion and CPU cost. OS window movement is still scheduled by
the desktop compositor, independently of pane rendering.

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

Use `with_notification_handler` to observe the same `DockspaceNotification`
as headless hosts and the single-surface egui adapter. Each notification combines
layout and native-placement summaries with detailed lifecycle events. Use
`notification.document_changed()` for debounced persistence: split resizing and
confirmed OS-window movement, size and native scale can request a save without
a detailed lifecycle event. The optional synchronous
callback receives only future nonempty publications, including native service
work and final UI passes. Pending transfers, discarded passes and rejected
actions do not produce completion notifications. The adapter retains no event
history. Callbacks run in commit order after internal settlement releases the
runtime lock. Shared runtime queries and queued commands are safe; keep handlers
short and schedule file I/O through application-owned workers. OS-window geometry
stays outside the docking-layout version. Unknown placement is not guessed;
first-live admission and geometry becoming authoritative again are also announced.
The callback's second argument, `NativeNotificationContext`, provides scoped
`with_view`, `request_command` and, with `serde`, `save_document_json` access.
Commands and saves follow the application's existing single-slot busy rules.

`pending_work()` explains `is_quiescent()` with an allocation-free snapshot of
current wait reasons. Inspect `contains(NativeWaitReason::Focus)` or iterate the
active reasons; debug formatting prints their names. This includes unconsumed
application results, pending presentations and native window retirement, while
ordinary stable repaint outputs remain idle. The query consumes no work and
retains no history. Inspect `output_backlog()` separately for output-owner counts.

`with_child_window_options` and `set_child_window_options` configure managed
Child window decorations, Windows taskbar presence, stacking level and icon.
Use `with_child_window_options_for(surface, options)` or
`set_child_window_options_for(surface, options)` for a stable `SurfaceId` override;
unset fields inherit the current global options. `child_window_options_for(surface)`
returns the resolved configuration. Overrides can be installed before creation
and survive replacement or restoration of the same surface.
`clear_child_window_options_for(surface)` removes an override and resumes the
global configuration. After inheritance, unset fields use the same defaults for
live windows and replacements: decorations and Windows taskbar presence are
enabled, the stacking level is `Normal`, and no window icon is set. Removing a
custom icon explicitly clears it; an unset icon does not inherit the Root icon.
`NativeChildWindowOptions::default()` keeps all fields unset so surface overrides
can inherit every global field. Changes apply on the next Root UI update, so
request a Root repaint when changing them outside an update. Creation and recovery
placement, visibility and ownership remain coordinated by the native lifecycle.
Applications may request ordinary movement of a live window through the host;
fresh OS observations determine its actual geometry. Configure the external
Root window through eframe's `NativeOptions::viewport`. Child titles follow selected panes
without adding a library-brand suffix.

Window ordering has two scopes:

- `NativeCommand::RaiseContained` raises a floating root inside its logical
  surface. It does not change OS window order.
- `NativeCommand::SelectItem` selects a pane and requests activation of its
  native window. Actual focus remains a platform observation.
- `NativeChildWindowOptions::window_level` requests `Normal`, `AlwaysOnTop` or
  `AlwaysOnBottom` globally or for one overridden surface. The Root uses its own
  viewport builder. A level does not specify order between windows at that level.

The adapter currently has no relative OS-window ordering API. `Child` describes
lifecycle ownership; it does not establish a native owner/transient-parent
relationship or guarantee that a child stays above the Root. Those policies
require explicit platform support.

[Examples](../../docs/EXAMPLES.md) · [Development](../../CONTRIBUTING.md)
· [Native qualification](../../integration/egui-native-smoke/README.md)

## License

MIT OR Apache-2.0. See [third-party sources](../../THIRD_PARTY.md).
