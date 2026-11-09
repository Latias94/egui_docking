# Observed pane-focus cue

This follow-up implements the active-pane visibility improvement from the
[egui UI audit](2026-11-09-egui-ui-style.md). The selected tab now has a thin
underline when its pane owns observed content focus. Selection, tab keyboard
focus, and content focus keep separate meanings.

| Before | After | Why |
| --- | --- | --- |
| Selected tabs in different panes look the same while one editor receives input. | The selected tab of the observed focused pane gains a two-point underline. | Users can identify the input destination without changing pane selection. |
| Keyboard tab navigation and editor focus have no distinct group-level treatment. | Tab keyboard focus keeps its outline; the content-focus underline disappears when focus moves onto chrome. | Navigation focus and content input are different states. |
| Applications need to inspect several style tokens to theme an active-pane cue. | The underline follows the local egui selection stroke, with `pane_focus_color` as an optional override and transparent color as an opt-out. | The default belongs to egui's existing visual language. |

## Focus and rendering contract

`SurfacePaintPlan::focused_pane()` reads the existing core focus coordinator's
current observation for that exact surface. It does not read selection, pending
requests, per-surface restoration history, or contained-window z-order. No new
focus state machine, command, or receiver is added.

Only an enabled, selected, visibly painted tab in a focused viewport receives
the cue. The underline stays inside the visible tab rectangle and is suppressed
for source gaps. The indicator does not request focus, change selection, move
content, or alter interaction bounds. Pane focus remains subject to the host's
observation and presentation contracts.

Applications expose an exact `PaneView::focus_target`, or an authoritative
composite `focus_state`, through the existing integration. The ordinary `basic`
example now exposes a stable editor identity. Applications can override
`style.visuals.pane_focus_color`, or use `Color32::TRANSPARENT` to hide the cue.

## Verification

Core regression assertions distinguish a pending pane-focus request from an
actual focused observation. Adapter regressions render real egui passes and
inspect paint shapes, framework focus, and selection. They cover focus transfer
between two split panes in the same root and a contained floating pane, tab
keyboard focus, local theme colors, explicit overrides, transparent opt-out,
window blur, and an unknown widget that cannot revive pane-focus history.

Independent read-only standards and behavior reviews found no P2 or higher
issue in the cue implementation. The following checks passed with one Cargo
build job, one nextest test thread, and the existing target directory:

| Check | Result |
| --- | --- |
| Main workspace, all features and targets | 2,278 passed, 3 skipped. |
| Official-egui public product harness, all features and targets | 119 passed. |
| Fork/native workspace, all features and targets except `egui-native-smoke` | 581 passed, 1 skipped. |
| Main and fork/native correctness/suspicious Clippy gates | Passed. |
| Strict core and adapter documentation | Passed with `RUSTDOCFLAGS=-D warnings`. |
| Workspace documentation tests | 11 passed. |
| Main and fork/native formatting | Passed. |

## Native startup focus gap: confirmed P2, still open

The real-window follow-up exposed an existing native startup observation gap.
In dark, light, and zoom 1.5 fixtures, clicking Document.rs's editor after
startup gives it egui keyboard focus and its normal editor border, but no
content-focus underline. Moving OS focus to a separate `xmessage` window and
back causes the underline to appear without changing the selected document.
After that fresh observation, the cue follows focus between the docked editor,
the contained Inspector, and external native egui content correctly.

The source supports the observed failure mode:

- `NativeGlobalFocusRecord::capture` freezes the callback-time route. If the
  root route or its committed visible roster is not ready, a positive native
  focus event becomes `Unknown`.
- Root registration binds the physical window later; committing its visible
  snapshot makes future focus callbacks reportable. Neither operation requests
  a fresh global focus observation.
- The pinned eframe fork emits `on_global_focus` after native `Focused` events.
  Clicking an editor inside the same OS window does not create another such
  event. Core correctly refuses to turn a pane claim into current global pane
  focus while native global focus remains unknown.

This change preserves that authority boundary. It does not make selection or
raw egui focus stand in for native global focus, cache and reinterpret an old
window event after rebinding, or request OS activation as a workaround. The
existing native regression
`exact_dock_focus_requires_the_last_committed_visible_roster` continues to pass.

A correct follow-up should emit a fresh provider focus snapshot at the root
roster/output boundary, with its observation order and exact window identity.
Native mapping can then use the current committed visible binding while still
rejecting hidden, retired, and rebound windows. The fork already has an
internal `focused_native_viewport()` query, but its public window snapshot and
roster omit focus and expose no request for a fresh observation. That is the
required integration seam; a native-only cache would add lifecycle logic
without the fresh evidence it needs.
Acceptance should include first-launch editor focus without an OS focus
round-trip, fresh observations after child creation or visibility changes,
and continued rejection of observations for hidden, retired, and replaced
bindings. The integration should use the existing focus reducer checks rather
than adding a second lifecycle resolver.

## Real-window evidence

The fixture reuses the pinned egui/native dependencies and layout from the
[previous UI verification](2026-11-09-egui-ui-fixes.md), adding stable pane
editor IDs and recording rectangle fills. The capture driver checks actual
X11 clicks, selection preservation, content focus distinct from tab focus,
and the number and placement of theme-colored underlines. Its explicit OS
focus round-trip is a diagnostic positive control for the open P2, not a fix.

| Case | Capture |
| --- | --- |
| Native startup editor focus without the missing observation | [Initial gap](2026-11-09-egui-pane-focus/evidence/dark-editor.png) |
| Docked editor after a fresh OS focus observation | [Dark](2026-11-09-egui-pane-focus/evidence/dark-refocused.png), [light](2026-11-09-egui-pane-focus/evidence/light-refocused.png), [zoom 1.5](2026-11-09-egui-pane-focus/evidence/zoom-refocused.png) |
| Focus moves to contained Inspector | [Floating pane](2026-11-09-egui-pane-focus/evidence/dark-floating.png) |
| Focus leaves dockspace for ordinary egui content | [External editor](2026-11-09-egui-pane-focus/evidence/dark-external.png) |

The [receipt](2026-11-09-egui-pane-focus/receipt.json) records tested source
hashes, exact commands, archived artifacts, and scope. All 14 node/text/shape
snapshots are retained in
[frames.json.gz](2026-11-09-egui-pane-focus/frames.json.gz). The
[fixture source](2026-11-09-egui-pane-focus/fixture/src/main.rs),
[manifest](2026-11-09-egui-pane-focus/fixture/Cargo.toml),
[lockfile](2026-11-09-egui-pane-focus/fixture/Cargo.lock), and
[capture driver](2026-11-09-egui-pane-focus/capture.py) remain outside the
production workspace. The driver writes `reproduced-evidence` when rerun and
expects the binary under `/native-target/debug`. Execution used Linux X11,
Openbox, Xvfb, XTest, and software Glow. Other native backends were not visually
qualified. No performance comparison was made.

## Floating-header consolidation assessment

The two contained-floating header rows currently carry distinct semantics.
The floating title drag region starts `MoveWindow`; the tab-group grip starts
`DockContent`. Group close and tab close also differ. The presentation menu
anchor belongs to the floating title and has mutually exclusive bounds with
its move and close controls.

Core compilation includes the title in contained minimum-size calculations and
lays out the root below it. Validation requires the title and root content to
be disjoint. An adapter-only removal leaves unused space; painting tabs upward
would disagree with the input geometry. The native example and real input
qualification also depend on title movement and menu commands.

A correct consolidation should therefore be a separate core layout change.
Only a single tabs root with a usable visible strip should receive a shared
header; split roots, empty roots, and hidden strips keep their separate title.
The shared row must preserve distinct window movement, whole-group docking,
single-tab close, group close, and presentation-menu controls, with a defined
narrow-width fallback. Existing workspace commands and persisted layout need
no additional state. Geometry, minimum sizes, validation, and public paint
records must change together, followed by core geometry, public input, and
actual native movement/dock-back regressions.

This assessment found no additional P2 in the double-header behavior. The
potential vertical-space saving does not justify silently removing those
existing operations as part of a focus-cue change.
