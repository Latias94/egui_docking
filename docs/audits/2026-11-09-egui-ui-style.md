# Egui adapter UI, theme, and docking ergonomics audit

The adapter fits egui's default light and dark themes reasonably well. Its pane
backgrounds, contained-window frame, shadows, and ordinary controls share egui's
visual language. The remaining problems are concentrated in text layout, popup
styling, and the connection between tab navigation and visibility. Five P2
defects were reproduced. This audit changes no production behavior.

The five defects and the popup/text-theme corrections have since been
implemented; see the [fix verification](2026-11-09-egui-ui-fixes.md). The
baseline captures and receipt below remain unchanged.
The [pane-focus follow-up](2026-11-09-egui-pane-focus.md) implements the active
content cue and records a separately reproduced native startup focus gap.

The recommended direction is a focused adapter cleanup plus two core tab-strip
corrections. The evidence does not justify replacing the docking architecture or
copying ImGui's palette.

## Evidence and comparison scope

The audited repository revision is `99b1841a18c15f19e365713d657efdee9aab874a`.
The reference is Dear ImGui's docking branch, `1.93.1 WIP`, revision
`d88f238d1d363ef387382b2e29ad12cf96f826ac`. Egui is the project's pinned
`0.37.0` fork at `f94ee58b3914de7ad71dab36919b329ef36b4035`.

Three independent read-only source reviews covered egui styling, public style
configuration, and ImGui docking behavior. The follow-up executed real GUI
fixtures on Linux X11 with Xvfb, Openbox, XTest input, and software OpenGL.
The egui fixture uses `NativeDockspaceApp`, `DockStyle::default()`, and native
egui controls in the same window. The ImGui fixture uses its actual docking
implementation and OpenGL backend. The final capture produced 34 frames; 24
representative screenshots are retained.

The fixtures have different fonts and content implementations. They support
qualitative design comparisons, not pixel-equivalence or performance claims.
The ImGui fixture forwards mouse input and Escape; its other keyboard behavior
is compared through pinned source. Pane content in these diagnostic fixtures has
deliberately simple padding and scrolling; content overflow is not classified as
an adapter defect.

| Baseline | Egui adapter with native egui controls at right | ImGui docking reference |
| --- | --- | --- |
| Dark | [Screenshot](2026-11-09-egui-ui-style/evidence/egui-dark-base.png) | [Screenshot](2026-11-09-egui-ui-style/evidence/imgui-dark-base.png) |
| Light | [Screenshot](2026-11-09-egui-ui-style/evidence/egui-light-base.png) | [Screenshot](2026-11-09-egui-ui-style/evidence/imgui-light-base.png) |

The [receipt](2026-11-09-egui-ui-style/receipt.json) records revisions, source
hashes, fixture settings, and screenshot hashes.
[Observations](2026-11-09-egui-ui-style/observations.json) contain the checked
focus, selection, glyph, and clip facts. The complete captured egui node and
text-shape records are in [frames.json.gz](2026-11-09-egui-ui-style/frames.json.gz).

## Confirmed P2 defects

| Finding | Before | After: recommended behavior | Why |
| --- | --- | --- | --- |
| P2-1: hidden tabs cannot be reached with End | In a narrow strip, End leaves focus and selection on `Document.rs` although the last tab exists. | Resolve the destination from the complete member roster, validate its eligibility, select it, and reveal it while retaining tab focus. | Navigation must continue across the visible boundary. |
| P2-2: resizing can hide the selected tab | Select the last tab in a wide window, then narrow the host; its content remains visible but its tab disappears. | Reveal the selected tab when the viewport shrinks, while allowing deliberate scrolling away afterward. | The selected document needs a reachable, visible tab after resizing. |
| P2-3: tab truncation loses its ellipsis | Text is elided for a 220-point width, then clipped into a 174-point text area in a fully visible closeable tab. | Elide against the final allocated text width and use a separate full-title layout for the menu. | The visible title currently looks abruptly cut off and loses distinguishing suffixes. |
| P2-4: floating resize edges have invisible keyboard focus | Tab can focus a resize edge; Right changes its position by 16 points without a focus marker. | Paint a visible indicator for the focused edge using egui interaction colors. | Keyboard users need to know what the next arrow key will change. |
| P2-5: a larger theme font clips title glyphs | A global 36-point font override leaves tab and floating-title heights at 28 points. | Provide an explicit egui-derived geometry recipe and synchronize the resulting geometry through the host boundary. | Font customization should produce readable titles without requiring users to discover several unrelated size fields. |

### P2-1: full membership and visible paint records disagree

Reproduction: resize the fixture to 700 by 620 client pixels, focus the
`Document.rs` tab through the real Tab order, then press End. The
[before](2026-11-09-egui-ui-style/evidence/egui-dark-end-before.png) and
[after](2026-11-09-egui-ui-style/evidence/egui-dark-end-after.png) captures retain
the same selected item and focused widget. The hidden last tab is absent from
the visible tab nodes.

Positive controls distinguish this failure from missing keyboard delivery:
[Right selects the visible History tab](2026-11-09-egui-ui-style/evidence/egui-dark-next-visible.png),
[Home returns to Document.rs](2026-11-09-egui-ui-style/evidence/egui-dark-home-visible.png),
and [End succeeds when all tabs are visible](2026-11-09-egui-ui-style/evidence/egui-dark-end-visible.png).

The destination comes from the complete roster in
[tab_strip.rs](../../crates/dockspace/src/tab_strip.rs#L18), but
[prepare_tab_navigation](../../crates/dockspace/src/runtime/paint/actions.rs#L322)
requires a visible destination record at line 349. A hidden destination
therefore returns no action. The correction belongs in core; the adapter should
not synthesize a second tab-navigation model. Keep the prepared action's exact
membership and policy checks.

### P2-2: geometry changes do not reveal the current selection

The [wide capture](2026-11-09-egui-ui-style/evidence/egui-last-base.png) selects
`Material settings and texture references`. After narrowing,
[the content still shows that document while the tab is hidden](2026-11-09-egui-ui-style/evidence/egui-last-narrow.png).

[The compiler](../../crates/dockspace/src/scene_compiler/surface.rs#L1276)
reveals the selected range only when the resolved selection changes. A viewport
change alone retains the old offset. ImGui explicitly tracks shrinking bar width
and requests the selected tab at `imgui_widgets.cpp:10109` and `:10259`.

Track the relevant published viewport change directly. Do not force the
selection into view every frame: that would break intentional scrolling to
other tabs. This correction needs a core resize regression and a real adapter
keyboard/focus check.

### P2-3: the shaped width and final text width are different

The isolated two-tab fixture leaves the entire long tab inside the viewport.
At zoom 1.0 its text clip is exactly 174 points wide, but the ellipsis starts
202 points after the text origin, completely outside that clip. At zoom 1.5
the same defect remains, with a 174-point logical text area and the ellipsis
starting about 201.33 points after the origin. See the
[default-scale capture](2026-11-09-egui-ui-style/evidence/egui-long-base.png) and
[zoomed capture](2026-11-09-egui-ui-style/evidence/egui-long-zoom-base.png).

[Measurement](../../crates/egui_dockspace/src/product_render/measurement.rs#L275)
elides a galley at `tab_max_width`. The
[compiler](../../crates/dockspace/src/scene_compiler/surface.rs#L1248) treats
that value as the maximum total tab width, including padding and close controls.
[Painting](../../crates/egui_dockspace/src/product_render/tabs.rs#L394) clips the
original galley into the smaller text rectangle. Proportional shrinking can
reduce that rectangle further.

Preserve the source text/layout job and shape the displayed title for its final
text allocation, using egui's text cache. Reuse a fitting galley without extra
work. Preserve clipping at the viewport for tabs that are genuinely partially
scrolled out. A menu row needs its own width decision and a complete-title
tooltip; it should not inherit a tab's already-elided galley. ImGui's actual
[window-list menu](2026-11-09-egui-ui-style/evidence/imgui-dark-menu.png) displays
the full long title, and its tab label implementation elides after accounting
for controls at `imgui_widgets.cpp:11268`.

### P2-4: keyboard resizing has no visible target

The recorded focused node is `Resize floating right edge` in both the
[before](2026-11-09-egui-ui-style/evidence/egui-dark-edge-before.png) and
[after](2026-11-09-egui-ui-style/evidence/egui-dark-edge-after.png) frames. Right
moves that edge from x=820 to x=836. There is no painted focus marker.

[Contained controls](../../crates/egui_dockspace/src/product_render/contained.rs#L239)
make cardinal edges focusable and accept keyboard adjustment but never paint
their focused response. The adapter's
[ordinary splitter](../../crates/egui_dockspace/src/product_render/splitters.rs#L48)
already supplies the appropriate local precedent. Preserve keyboard resizing
and add its missing visual feedback. Native egui Window resize edges are
non-focusable drag handles; this adapter intentionally offers more keyboard
capability, so it also needs an indicator.

### P2-5: font size and docking chrome height have independent defaults

With `Visuals` unchanged and `override_font_id` set to 36-point proportional
text, the floating `Inspector` title has a 41-point galley. Its actual glyph ink
extends 5.5 points below the fixed title clip. Both floating and tab captions
visibly lose part of their glyphs in the
[font-override capture](2026-11-09-egui-ui-style/evidence/egui-font36-base.png).
Native egui controls in the same window allocate taller geometry.

The two 28-point chrome heights come from
[core defaults](../../crates/dockspace/src/presentation_config.rs#L350), exposed
by [DockStyle](../../crates/egui_dockspace/src/style.rs#L155). The adapter reads
current fonts for text but does not derive initial chrome geometry from them.
ImGui uses font height plus frame padding for its tab bar and tab sizes at
`imgui_widgets.cpp:9989` and `:11155`; the
[larger-font reference](2026-11-09-egui-ui-style/evidence/imgui-large-base.png)
shows its taller caption strip.

This is a default ergonomics gap under font customization. An application can
currently compensate by explicitly configuring matching geometry. The fixed
geometry contract itself remains useful: offer one explicit style-derived
initialization/update path, and keep that resolved geometry stable throughout a
frame. Native construction must continue to check style/session agreement.

Whole-UI zoom is a separate mechanism. The
[zoom-only capture](2026-11-09-egui-ui-style/evidence/egui-zoom-base.png) preserves
the logical font-to-title-height ratio. Do not multiply docking point geometry
by zoom a second time. Its narrower logical viewport also exposes the existing
horizontal truncation behavior; that is not a new DPI defect.

The diagnostic `Inspector` also falls outside the smaller dock area in some
narrow/zoom captures because its saved position is x=440. This follows the
existing durable-placement contract: passive resize preserves that rectangle,
and [bring_contained_into_view_current](../../crates/egui_dockspace/src/product_dockspace.rs#L557)
is the explicit recovery action. The minimal fixture omits the application's
window inventory/recovery controls. Its clipping is not evidence that recovery
is missing from the product.

## Theme consistency and optional ergonomic improvements

These items are lower priority than the five reproduced functional defects.
They do not all require public API changes.

| Before | After: recommended direction | Why |
| --- | --- | --- |
| The hidden-tab popup uses a fixed 3-point radius, 1-point border, and no popup shadow. | Resolve its frame from egui's popup/menu styling and use the same frame metrics for measurement and painting. | The presentation menu already follows those theme values; adjacent popup types should look related. |
| Tab and overflow text force widget foreground colors even with a global `override_text_color`. | Honor the global text override unless a docking-specific text override is explicitly set. | The custom-theme captures show magenta native controls and floating captions alongside gray/black tab text. |
| Menu row height includes `tab_horizontal_padding`; the current menu item uses the same background as ordinary rows in the default theme. | Derive row height from menu/font vertical metrics and provide a persistent selected-item marker independent of keyboard hover/focus. | Horizontal tab padding should not alter vertical menu density; moving focus should not erase the selected-item cue. |
| Two panes can each show a selected tab with no indication of which content editor owns focus. | Add a subtle active-pane cue based on the existing confirmed focus state. | ImGui distinguishes selected tabs in focused and unfocused groups; selection and keyboard focus carry different information. |
| A simple floating tab group displays a group title row plus a tab row. | Consider integrating those rows for a single tab-group root, preserving group drag, group close, and menu semantics. | It reduces duplicated captions and vertical chrome; split floating roots can retain a separate group title. |
| Child OS windows default to `Dockspace panel`. | Derive a descriptive title from the selected pane or group while preserving stable window identity. | Multiple child windows need distinguishable task-switcher names. This is a source-confirmed host improvement, not an exercised OS-title regression. |

The custom-theme comparison sets menu/window corner radius to 12, window stroke
to 3 points, and a global magenta text override. Compare the
[hidden-tab popup](2026-11-09-egui-ui-style/evidence/egui-sentinel-overflow.png),
[presentation commands](2026-11-09-egui-ui-style/evidence/egui-sentinel-commands.png),
and [native egui menu](2026-11-09-egui-ui-style/evidence/egui-sentinel-native-menu.png).
The latter two follow the popup frame recipe. The hidden-tab popup changes its
border color but keeps its fixed radius and width. Its
[implementation](../../crates/egui_dockspace/src/product_render/tab_chrome.rs#L266)
also lacks the native popup shadow. Measurement currently accounts for the
theme's stroke width while painting hardcodes another width.

In the [light-menu focus capture](2026-11-09-egui-ui-style/evidence/egui-light-overflow-next.png),
`History` has keyboard focus while `Document.rs` remains selected. The latter
has no distinct selected background. Both default themes use equal panel and
window fills, which are the two colors chosen for selected and ordinary menu
rows. A checkmark or a theme-native selected treatment would resolve this
without changing menu behavior.

The popup should retain its existing core-owned state, placement, input
authority, and dismissal rules. Reusing egui's frame and row styling does not
require creating a competing `Popup` lifecycle. Similarly, square tiled pane
edges suit a docking workspace; native egui button rounding is not a reason to
round every tiled surface or impose application content padding.

## Recommended implementation order

1. Fix core navigation across hidden members and selected-tab visibility after
   viewport shrink. Cover hidden End/Next, visible positive controls, deferred
   focus, and deliberate manual scrolling.
2. Correct final-width text elision, separate menu title layout from tab layout,
   and paint focused floating resize edges. Reuse existing egui galley caching
   and splitter interaction styling.
3. Consolidate egui-derived popup visuals and offer a small explicit recipe for
   font/spacing-derived docking geometry. Ordinary `Dockspace::set_style`
   already commits geometry atomically; any native runtime style update must
   honor the same single-authority contract instead of silently replacing a
   live session's configuration.
4. Revisit active-pane emphasis and optional floating-header consolidation after
   the behavior and theme fixes. Add dark/light, larger-font, zoom, and narrow
   variants to an existing example for ongoing manual inspection.

Keep selection and pane-content focus separate, preserve the explicit native
window commands, and retain the current core-owned drop guides and hysteresis.
No new evidence in this audit requires another docking-state model, a generic
theme engine, a validation framework, or an additional performance benchmark.

## Reproduction artifacts and validation

[The egui fixture](2026-11-09-egui-ui-style/fixture/src/main.rs),
[its manifest](2026-11-09-egui-ui-style/fixture/Cargo.toml),
[the ImGui fixture](2026-11-09-egui-ui-style/fixture/imgui_audit.cpp), and
[the capture driver](2026-11-09-egui-ui-style/capture.py) are audit artifacts,
outside the production workspace. Archived dependency and output paths are
rebased; source behavior and pinned dependencies are preserved. The lockfile is
included. The capture driver expects X11 utilities and the two fixture binaries
under `/native-target`; it is not a new project test command.

The egui fixture was built with one Cargo job and the existing target directory.
Both fixture builds and the final 34-frame capture completed successfully.
Assertions checked keyboard positive controls, popup opening, focused resize
motion, hidden selection, and the measured glyph/clip relationships. Those
assertions document the current defects; they are not passing production
regressions for a fix. Production tests and performance comparisons were not
rerun because production code did not change. Linux X11 is the executed backend;
this audit does not establish equivalent results on other window systems.
The [build log](2026-11-09-egui-ui-style/final-build.log.gz) and
[capture log](2026-11-09-egui-ui-style/final-capture.log.gz) are retained.
