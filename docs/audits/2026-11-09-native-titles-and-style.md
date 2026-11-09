# Native child titles and runtime styling

This follow-up starts from `e619124`. Managed OS children now describe their
selected panes, and native applications can update docking geometry and visuals
without reconstructing their session or windows.

## Child captions

Each child caption includes the selected pane from every tabs leaf in its main
and contained roots, in stable traversal order. A single Inspector group becomes
`Inspector — Dockspace`; a split group can become `Inspector · Preview — Dockspace`.
Missing or empty pane captions fall back to the item identity. An empty or
not-yet-published surface uses `Dockspace panel <surface>`.

Captions enter the existing deferred viewport builder, whose ordinary diff
emits OS title updates. The title does not participate in viewport or binding
identity. A semantic child callback requests a Root declaration refresh when
its pane title differs from that declaration. Stable titles converge without
repeated OS commands. Explicit application title commands remain effective
until the derived pane caption changes; the external Root title stays with the
application. No title cache or second lifecycle model was added.

The editor driver still qualifies PID, visibility, exact window identity and
destruction. It now compares Unicode captions in Python instead of relying on
the search tool's locale-sensitive regex engine. Real OS checks confirmed
Inspector and the simultaneously live Inspector/Assets children.

## Style interface and publication

`NativeDockspaceApp::request_style(DockStyle)` shares the existing bounded
application command slot. Invalid requests preserve their configuration error
source; Busy and Stopped retain the existing queue errors. The request and its
`NativeActionStatus::StyleChanged { changed, geometry_changed }` result occupy
that slot until the application consumes the terminal status.

`DockPresentationConfig::to_builder` preserves the current complete core
configuration. `DockStyle::presentation_config_with` applies only the adapter's
represented fields through the same mapping as initialization. Runtime updates
therefore retain custom pointer thresholds, scroll-line extent, magnetic
targeting and release hysteresis.

The style travels with the existing unpublished native update. Encoding and
repeated passes keep the old style and core configuration. After the final
core commit, the adapter checks the configuration input's exact submitted
receipt and publishes its style under the same runtime lock, before later
fallible host settlement. Dropped candidates publish neither configuration.

Geometry changes use core's existing revision, gesture cancellation and fresh
measurement rules. A visual-only or equal style skips configuration replacement;
even an equal replacement input could otherwise cancel a pending presentation
obligation. Every successful request repaints all mapped viewports. Equal style
values still need this repaint when egui fonts or theme resources change.
The request creates no queue, timer, native binding or OS window.

The native example's Appearance menu derives metrics after font or theme
changes and waits through its existing action-result and presentation flow.
The menu closes on outside clicks so its numeric font editor remains usable.
Real X11 input changed the font from 13 to 28 points; the resulting caption
geometry and the before/after captures were visually inspected. Saved contained
placement remains durable, and application content layout remains application-owned.

## Compatibility

- Initialize native sessions with the matching `style.presentation_config()` as
  before. Use `request_style` for subsequent updates and consume its terminal
  result before waiting for presentation or quiescence.
- Egui continues to own font resources and global theme state. Reapply
  application-specific overrides to `DockStyle::from_egui(ui)` before queuing it.
- `DockspaceHostFrame::replace_presentation_config` now returns a
  `DockspaceSubmittedAction` receipt. Callers may ignore it, or use
  `HostFrameReport::submitted_action_outcome` to obtain the exact
  `PresentationConfigReplaced { changed, revision }` result. Helpers with an
  explicit `Result<(), _>` return must map a successful receipt to `()`.

## Verification

All Cargo builds used one build job and reused the existing target directories.
Nextest used one test thread. Correctness and standards reviewers found no
remaining P2 or higher issue after the equal-style repaint correction.

- Main workspace, all features and targets: 2,279 passed, 3 skipped.
- Native package, all features and targets: 590 passed, 1 skipped.
- Native package, default features and all targets: 579 passed, 1 skipped.
- Core and native correctness/suspicious Clippy gates, strict documentation,
  workspace documentation tests and formatting passed.
- Real Glow tests verified both ordinary and style-changing multipass updates,
  final-only publication, retained-output backpressure and subsequent recovery.
- The real command journey retained its nine docking operations and added four
  live-child style updates: geometry, visual-only, equal and restored defaults.
  It checks child content height, exact window identity and pane data. The equal
  request changes egui's global text color; the driver waits for the actual
  child pane callback to observe it before continuing.
- The full editor journey passed 12 real edits, six child lifetimes, document
  replacement, Root continuity and final quiescence, with actual OS caption checks.

The [receipt](2026-11-09-native-titles-and-style/receipt.json) records source
hashes, selected logs and screenshots. Visual qualification covers Linux X11,
Openbox, Xvfb, XTest and software Glow. This run does not add Windows, Wayland,
macOS or WGPU real-window visual qualification, or a new performance claim.
