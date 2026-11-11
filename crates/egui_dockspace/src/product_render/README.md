# Product renderer pointer ownership

This directory renders the ordered `SurfacePaintPlan` produced by the core. The
host transports ordered pointer input through its journal before measurement and
paint. The renderer publishes widget facts and draws core-owned interaction
visuals. This document describes the contracts required by the host adapter.

## Rendering and input

Core input reduction owns gesture state, drag thresholds and release. The
renderer executes the resulting paint plan and registers egui widget facts.
Semantic button, keyboard and accessibility actions remain separate commands.
`ctx().dragged_id()` is consulted only to release egui drag ownership for
journal-owned docking widgets.

## Authority contract

| Authority | Local pointer actions | Preview support |
| --- | --- | --- |
| `HostJournal` | false | true |
| `ExternalJournal` | false | true |
| `LocalResponses` | true | false |

`HostJournal` is available with every feature configuration. `ExternalJournal`
requires `native-render-support`. `LocalResponses` keeps semantic button actions
available when the enclosing host owns terminal output and cannot supply a
settlement receipt. Preview support controls presentation and focus handling; it
does not authorize response-derived gesture emission.

The ordinary host drains and disables its pointer producer before painting when
presentation settlement becomes unavailable. Core retirement cancels the exact
active interaction at that boundary. The renderer does not retain cancellation
history or enqueue a second authority-loss action.

For both journal authorities, `interact_receiver` releases egui drag ownership
when the registered widget currently owns it. The widget registration and completed
pass hit record remain available to the host.

## Host frame and presentation contract

The host owns the following ordering outside this directory:

1. Observe presentation results and associate accepted artifacts with the exact
   receiver declarations emitted alongside them.
2. Qualify ordered pointer events against that accepted output and submit them to
   the core journal before measurement and paint.
3. Measure when required, then obtain the resulting core paint plan.
4. Execute that plan, retain widget declarations, and collect semantic actions.
5. Confirm a complete execution receipt and settle the emitted output through the
   host-owned terminal egui run.
6. Submit current-pass semantic actions at their separate core boundary without
   changing the interaction represented by the executed paint plan.

A candidate plan, a registration, and an emitted output each have distinct roles.
An emitted output becomes pointer-routing authority only after its matching
presentation is accepted. Receiver declarations must remain associated with the
exact artifact; matching geometry alone does not establish freshness.

Both drag preview and contained-transform preview set `defer_measurement` after
successful paint when `supports_preview_gestures()` is true. This includes
`HostJournal`: preserving a ready target during pending release must not depend on
local gesture emission. The host can still measure when no ready plan exists.

## Receiver declarations and blockers

`ProductReceiverBinding` and `ProductPaintOutput::receivers` are available in every
feature configuration. Each declaration contains:

| Field | Meaning |
| --- | --- |
| `widget_id` | Exact egui response identity |
| `layer_id` | Layer recorded by that response |
| `interact_rect` | `response.interact_rect`, including egui clipping |
| `interactive` | `sense.interactive() && response.enabled()` at registration |
| `receiver` | Opaque descriptor supplied by the current core paint plan |

These facts let the host conservatively distinguish registered docking chrome
and explicitly declared pane content from unknown application widgets in
`interactive_rects_last_pass()`. A host must only exclude the corresponding
occurrence of a known interactive rectangle after checking its exact widget,
layer, enabled state, and interaction rectangle in `prev_pass.widgets`. Each
widget ID can remove at most one occurrence. The output's completed pass must
match the previous pass exactly; a newer unpresented pass cannot lend its facts
to an older accepted output. Non-identity emission or input layer coordinates
cancel the pointer stream rather than supplying raw manipulation coordinates
to an existing gesture.
Overlapping equal rectangles can represent different widgets. A hover-only or
disabled declaration must not remove an unknown application interactive rectangle,
even when their bounds are identical.

### Child UI layout precision

Core geometry remains continuous. Its paint bounds, receiver descriptors, clips
and pointer positions are not snapped to egui's layout grid. When constructing a
pane content Ui or a splitter-junction accessibility Ui, the adapter fits only
that child's layout rectangle inside the semantic region on `GUI_ROUNDING`
boundaries. Each edge moves inward by less than one grid step (1/32 logical
point), so application widget declarations remain inside the original pane.
Regions without a positive grid-aligned interior do not create a child Ui;
their core-owned background and pointer receivers remain available.

The debug `show_unaligned` overlay stays enabled. Adapter regressions inspect
the emitted shapes for `Unaligned` labels at several scales, including fractional
split and contained bounds. The actual editor docking smoke also rejects these
labels while exercising real input.

`guides.rs` registers drop guides with `Sense::hover()`, so their bindings have
`interactive == false`. The accessibility label for the center guide does not turn
it into an interactive widget. The `DropTarget` program step itself does not create
an egui interactive control. Expected delivery and hover receivers come from the
core's `host_pointer_hit` query; the adapter does not reconstruct drop-target
policy, eligibility, or priority from these declarations.

Scroll receiver registration remains behind `native-render-support`, including
`scroll_registrar` and `ProductScrollReceiverBinding`.

### Explicit pane content contract

`PaneView::ui` returns a `PaneResponse`, an application-owned declaration of
ordinary content registered during that callback. The adapter reads its IDs
immediately, while the child UI is still alive. No separate declaration getter
or per-item ID cache is retained. An empty response declares nothing. An undeclared
or rejected widget remains an unknown blocker; the adapter never walks the widget
tree to discover content ownership.

For each declared ID, `tabs::paint_pane` reads `this_pass.widgets` and accepts only
an enabled registration on the child UI's exact layer. This includes explicitly
declared hover-only labels and ordinary content containers. Its already-clipped
`interact_rect` must be finite, positive, and fully contained within the pane's
finite, positive content bounds. The adapter preserves that exact interaction
rectangle rather than intersecting an out-of-bounds response into eligibility.
Each accepted fact becomes a `ProductReceiverBinding` carrying the exact
`receiver_for_pane(pane)` descriptor from the current paint plan, not a fabricated
or surface-wide receiver. Its `interactive` field retains the widget's actual
interactive sense. A hover-only declaration therefore lets the native adapter
qualify that exact frontmost identity for hover clearance, but cannot subtract an
unknown interactive rectangle in the official adapter or change click/drag sensing.

Declaring ordinary content permits docking hover to pass through it and allows
activation of its containing window, without replacing the widget's ordinary
egui input handling or turning it into a docking drag handle. Popup, modal, and
foreign-overlay controls must never be declared, even if they share the layer or
fit inside the pane. Geometry and `read_response` do not prove ownership or that
a widget was registered during the current callback: those are explicit host
application facts. This contract is not an automatic widget-tree resolver or a
general inference of platform hit eligibility. Existing presentation acceptance,
focus confirmation, output-preservation checks, and source-gap handling remain
separate requirements.

Composite content needs declarations for its actual registrations, including
hover-only content, not just a visual container ID. In particular, an ordinary
scroll region can contain an independent scrolling/drag response, a scrollbar,
and child widgets; each unknown registration still blocks. The official adapter
separately checks the top composed layer (including modal exclusion) before
subtracting known widgets, so declaring content does not authorize a popup or
foreground layer.
`integration/egui-official-harness/tests/hover_blockers.rs` exercises these real
composed blockers and stale-pass rejection through the public adapter, including
the case where an earlier clear preview must not survive a blocked release.
The native receiver tests in `crates/egui_dockspace_native/src/receiver.rs` also require
declared hover-only pane content to clear the exact presented hover lane while
undeclared content and foreign foreground overlays remain blockers.

## Preserved semantic input

The removed frontend concerns docking pointer gestures and contained pointer
activation. The following semantic entry points remain:

- Tab activation through keyboard or AccessKit; Left/Right/Home/End navigation
  when the tab widget has framework focus.
- Keyboard and AccessKit splitter and contained-resize adjustments.
- Semantic close actions, tab-strip controls, tab-list navigation, focus, reveal,
  dismissal, and scrolling.
- Presentation command menus and their ordinary egui popup interaction.
- Local pointer clicks gated by `accepts_local_pointer_actions()` for unowned
  `LocalResponses` callers.
- The tab-list scrollbar's response-driven pointer-to-scroll-offset conversion
  under every pointer authority, subject to its enabled state. Its drag ownership
  and grab anchor belong to the scroll subsystem, not the docking journal; it is
  a scroll semantic entry point, not a docking gesture-phase producer.
- Escape cancellation when the paint plan exposes the corresponding semantic
  action. Journal stream cancellation is transported by the host.

`actions.rs` therefore still translates button activation into opaque core actions.
It does not read the raw event stream or produce docking gesture phases.
Button activation preserves keyboard, AccessKit, then local pointer priority.
Pane focus separately retains its focused, unfocused, and unknown observations.

A policy-disabled visible tab keeps its visible bounds for hover and
accessibility. Its core drag region may be empty; that does not erase its visual
or accessible extent. The response remains hover-only, the accessible node stays
disabled, and neither can authorize a semantic action.

## Source gaps, cursors, and focus

`SurfacePaintDisposition` remains the source-gap authority. Widget identities and
pane callbacks survive omitted source pixels, preserving focus and accessibility
identity. Transient visuals follow the core paint program.

Tab and tab-group grabbing cues use the core source-gap disposition. The core
`DragDecoration` step also selects `CursorIcon::Grabbing`, independently of an egui
drag response. Resize handles retain hover cursor feedback. These paths do not
reconstruct active drag state from `Response::dragged()`.

`HostJournal` participates in the same pane output-preservation checks and pane
focus confirmation path as `LocalResponses`. Core pane focus requests
resolve through `PaneView::focus_target`; final focus observations remain tied to
the exact output's settlement.

The selected tab's content-focus underline reads
`SurfacePaintPlan::focused_pane`, which exposes the current core observation for
this surface. It does not infer activity from selection, floating z-order,
restoration history, or a pending request. Painting the cue never requests focus
or changes interaction bounds. The source-gap disposition, enabled strip state,
and current viewport focus suppress it when appropriate. Its color follows the
local egui selection stroke unless explicitly overridden.

Pane content focus and tab chrome focus are distinct. An accepted journal tab
click must preserve the intended framework focus behavior for subsequent keyboard
navigation. `capture_tab_actions` only requests tab widget focus for local pointer,
keyboard, or AccessKit activation. A host integration must bridge accepted core
tab activation to the matching widget identity if that behavior is required; an
unqualified raw click is not sufficient evidence. This bridge must not introduce a
second gesture state machine or override a core request to focus pane content.

Keyboard tab navigation keeps framework focus on its destination tab after the
queued selection commits, including when that pane exposes an editable focus
target. The opaque core action preserves this navigation purpose through direct
and deferred submission and supersedes older content-focus requests using the
existing focus coordinator. Enter, Space and explicit tab activation retain their
ordinary pane-content focus behavior.

Escape dismisses an open tab-list menu and returns keyboard focus to its owning
tab-list button when that button remains enabled and visible. Repeated encoding
passes in the same run preserve that handoff, so Enter can reopen the menu after
the queued dismissal commits. Pointer backdrop dismissal keeps its existing
focus behavior.

## Validation and regression diagnosis

`pointer_authority_contract_tests` covers the three concrete renderer modes.
Discard-run tests and native preview tests cover their separate presentation
responsibilities. The public tab-accessibility regression checks policy-disabled
tabs through both callback-only and host-owned terminal runs.

Owned pointer tests require an armed host input run, an accepted presented output,
and receiver facts from that exact output. A synthetic egui click or a painted
candidate alone does not satisfy the journal contract. For missing previews,
inspect accepted output identity, qualified delivery/hover facts, journal reduction,
and the core preview result in that order. For keyboard regressions after a
journal click, also inspect framework focus on the tab widget before navigation.
These are diagnostic prerequisites, not a conclusion about any particular failing
fixture.

Renderer cleanup can be checked with direct `rustfmt`, `git diff --check`, and
workspace searches for the removed symbols. Cargo builds and product integration
tests remain the responsibility of the main integration owner.
