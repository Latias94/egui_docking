# Egui docking UI fixes and verification

This change implements the five P2 corrections from the
[UI and ImGui comparison](2026-11-09-egui-ui-style.md), together with its popup
and text-color consistency recommendations. The docking core still owns tab
membership, selection, scrolling, popup state, and input eligibility. The egui
adapter derives visuals and text layout from the current host style.

## Behavior changes

| Before | After | Why |
| --- | --- | --- |
| End cannot select a hidden last tab. | Navigation uses the complete strip membership, selects and reveals its destination, and retains keyboard focus on that tab. Home and wrapping arrows follow the same path. | Visible paint records are an incomplete navigation roster. |
| Narrowing the viewport can hide the selected tab. | The first installed plan after a viewport shrink reveals the selection. Subsequent manual scrolling remains effective. | Automatic recovery should react to the geometry change without continuously overriding the user. |
| A 220-point closeable tab shapes its caption wider than its final 174-point text allocation, clipping away the ellipsis. | Captions elide at the final text allocation. Scrolling clips the resulting galley without moving its original text position. Menus and drag ghosts use their own allocations. | Text allocation and viewport clipping serve different purposes. |
| A focused floating resize edge accepts arrows without a focus indicator. | The focused cardinal edge has a visible egui interaction-color marker; arrow resizing retains the same target. | Keyboard users need a visible indication of the operation they are about to perform. |
| A 36-point font retains fixed 28-point caption geometry. | `DockStyle::from_egui(ui)` derives matching font, padding, interaction, and border geometry. The ordinary `basic` example refreshes it after font changes. | Hosts need one explicit font-aware geometry recipe. |
| The open-tab menu hardcodes its frame and mixes tab padding into row height. | Measurement and painting share egui's popup frame, margins, rounding, stroke, and shadow; rows use font height and vertical button padding. | The menu should follow local egui styling and use the same measured and painted geometry. |
| Global text overrides omit tab captions, and moving menu focus erases the current-item cue. | Captions and menu text honor explicit docking overrides before the global text color; selected rows retain the theme selection treatment. Disabled strip controls use egui's disabled alpha. | Theme customization and selection should remain consistent across adjacent controls. |

## Implementation and API

Core navigation now distinguishes activation from navigation with an explicit
source and destination. The visible, operable source supplies input authority;
the destination must belong to the same enabled strip. Hidden destinations do
not need a visible paint record. The prepared action exposes a stable target
visual so egui can request the destination widget's focus before the next frame
reveals it. Ordinary activation still transfers focus to pane content.

Deferred navigation retains its frozen root/membership and strip-geometry
checks. Reordering members after preparing an action rejects the old action
without changing scrolling. Navigation to an already-selected, manually hidden
tab also explicitly reveals it.

The transient tab-strip state records the last installed viewport width.
Preparing a candidate, rejecting a stale candidate, or retaining old output
cannot adopt a new width. `TabRecord::text_layout_bounds` and the matching
runtime accessor separate the full text allocation from the existing visible
`text_bounds`. Egui reuses a fitting galley and uses its text cache when the
final allocation needs elision. The old normal-paint tab-visual lookup map is
now only materialized by opt-in native diagnostics.

`DockStyle::from_egui(ui)` is an explicit snapshot of the current font and
spacing metrics. Use the result with the ordinary builder or
`Dockspace::set_style`, reapplying application overrides as needed. Changing
whole-UI zoom does not require multiplying logical geometry again. The
[adapter README](../../crates/egui_dockspace/README.md#styling) documents
the initialization and update contract.

Native hosts must create their session with that same style's
`presentation_config()` and pass the matching style to `NativeDockspaceApp`.
This change does not add a runtime native geometry-update command. Native
font-dependent geometry is configured at session creation; the native fixture
uses this path for its larger-font variants.

## Regression coverage and independent review

The new
[adapter regressions](../../crates/egui_dockspace/src/product_chrome_tests.rs)
exercise actual egui passes, AccessKit nodes, text shapes, and keyboard input:

- Hidden End/Home and wrapping arrows reveal their destination and retain tab
  focus across later frames; Enter then transfers focus to the editor, which
  accepts text input.
- Viewport shrink reveals the selected tab once, subsequent manual scrolling
  remains effective, and navigating back to the unchanged selection reveals it.
- Ellipses fit the final allocation at zoom 1.0 and 1.5.
- Locally customized popup frames, row geometry, full menu titles, and global
  text colors agree between measurement and paint.
- Explicit tab text overrides remain effective with keyboard focus.
- Floating-edge focus produces a painted marker, and Right moves the same
  edge by 16 logical points.
- Both a global font override and a customized Button font at 36 points fit
  tab and floating-title glyphs when using the derived geometry.

Core tests cover viewport-shrink recovery, adoption only from installed Ready
plans, and rejection of deferred navigation after a member reorder. The
existing public API test
`selected_tab_text_override_does_not_leak_into_close_interaction` caught a
shared-color-helper regression during implementation. The corrected code keeps
text override precedence separate from icon interaction strokes; the existing
test passes without weakening its assertion.

| Review axis | Final result |
| --- | --- |
| Standards and maintainability | Independent read-only review: no outstanding actionable findings; the final text/icon correction introduced no P2 or higher issue. |
| Audit requirements and behavior | Independent read-only review: no outstanding actionable findings; the final text/icon correction preserves the existing close-control contract. |

## Executed checks

All Cargo builds and test runs used one build job and reused the existing
target directory. Nextest execution used one test thread.

| Check | Result |
| --- | --- |
| Main workspace, all features and targets | 2,276 passed, 3 skipped. |
| Fork/native workspace, all features and targets, excluding `egui-native-smoke` | 581 passed, 1 skipped. |
| Official-egui public product harness, all features and targets | 119 passed. |
| Adapter, all features and targets, rerun after the text/icon correction | 155 passed. |
| Main and fork/native workspace correctness/suspicious Clippy gates | Passed. |
| Strict documentation for `dockspace` and `egui_dockspace` | Passed with `RUSTDOCFLAGS=-D warnings`. |
| Workspace documentation tests | 11 passed. |
| Main and fork/native workspace formatting | Passed. |

The [verification receipt](2026-11-09-egui-ui-fixes/receipt.json) records exact
commands, source hashes, artifact hashes, and scope. The
[validation log](2026-11-09-egui-ui-fixes/validation.log.gz) retains the final
suite and gate outputs. These are local results, not a claim about remote CI
or unexecuted platform-specific GUI jobs.

## Real-window results

The updated native fixture built successfully and completed all nine egui
variants, producing 30 captured frames. Sixteen representative screenshots are
retained. The checked captions, menu, selection, and focused-edge screenshots
show the expected behavior.

| Case | Original capture | Fixed capture |
| --- | --- | --- |
| Hidden last tab reached with End | [Before](2026-11-09-egui-ui-style/evidence/egui-dark-end-after.png) | [After](2026-11-09-egui-ui-fixes/evidence/egui-dark-end-after.png) |
| Selected tab after narrowing | [Before](2026-11-09-egui-ui-style/evidence/egui-last-narrow.png) | [After](2026-11-09-egui-ui-fixes/evidence/egui-last-narrow.png) |
| Ellipsis in a closeable tab | [Before](2026-11-09-egui-ui-style/evidence/egui-long-base.png) | [After](2026-11-09-egui-ui-fixes/evidence/egui-long-base.png), [zoom 1.5](2026-11-09-egui-ui-fixes/evidence/egui-long-zoom-base.png) |
| Floating-edge focus and Right adjustment | [Before](2026-11-09-egui-ui-style/evidence/egui-dark-edge-before.png) | [Focused edge](2026-11-09-egui-ui-fixes/evidence/egui-dark-edge-before.png), [after Right](2026-11-09-egui-ui-fixes/evidence/egui-dark-edge-after.png) |
| 36-point font override | [Before](2026-11-09-egui-ui-style/evidence/egui-font36-base.png) | [After explicit geometry derivation](2026-11-09-egui-ui-fixes/evidence/egui-font36-base.png) |
| Custom popup frame and text color | [Before](2026-11-09-egui-ui-style/evidence/egui-sentinel-overflow.png) | [Open-tab menu](2026-11-09-egui-ui-fixes/evidence/egui-sentinel-overflow.png), [native egui menu](2026-11-09-egui-ui-fixes/evidence/egui-sentinel-native-menu.png) |
| Selection after keyboard focus moves | [Before](2026-11-09-egui-ui-style/evidence/egui-light-overflow-next.png) | [After](2026-11-09-egui-ui-fixes/evidence/egui-light-overflow-next.png) |

End now leaves the hidden last tab selected, visible, and focused in the actual
native run. The resize scenario also keeps that selected tab visible. In the
isolated long-title fixture, the ellipsis ends 169.95 points into the 174-point
text clip at zoom 1.0, and 169.61 points into it at zoom 1.5. The previous
ellipsis started beyond 201 points, outside the clip. With the 36-point font
override and `from_egui`, caption height resolves to 45.375 points; the floating
title and tab glyphs both fit vertically. The focused floating right edge has a
visible marker before and after its 16-point adjustment.

The custom-theme popup has the same 12-point rounding, 3-point blue border,
shadow, and magenta text recipe as the native egui menu. Its long title fits
without reusing the shortened tab galley. In the light-theme menu, Document.rs
keeps its blue selected background while History has the separate keyboard
focus highlight.

[Observations](2026-11-09-egui-ui-fixes/observations.json) preserve the checked
numeric relationships and input targets. The complete node and text-shape
snapshots are in [frames.json.gz](2026-11-09-egui-ui-fixes/frames.json.gz).
The [GUI build log](2026-11-09-egui-ui-fixes/gui-build.log.gz) and
[capture log](2026-11-09-egui-ui-fixes/gui-capture.log.gz) record the executed
native fixture, including its final text/icon color correction.

## Reproduction and scope

The original [baseline evidence](2026-11-09-egui-ui-style/receipt.json) is
preserved. The follow-up reuses the same native layout and actual X11 input
steps, with passing assertions for hidden End selection and selected-tab
visibility after shrink. The updated
[fixture](2026-11-09-egui-ui-fixes/fixture/src/main.rs),
[manifest](2026-11-09-egui-ui-fixes/fixture/Cargo.toml),
[lockfile](2026-11-09-egui-ui-fixes/fixture/Cargo.lock), and
[capture driver](2026-11-09-egui-ui-fixes/capture.py) are audit artifacts outside
the production workspace. The driver expects the fixture binary under
`/native-target/debug` and writes a separate `reproduced-evidence` directory.

ImGui's pinned reference remains the one from the original audit. These fixes
follow egui's font, popup, spacing, and interaction conventions. They make no
new performance claim or pixel-equivalence claim against ImGui.

The optional active-pane emphasis, floating-header consolidation, and native
OS-window title improvements remain separate design work. Real-window
qualification here uses Linux X11 with Openbox, Xvfb, XTest, and software Glow;
it does not establish equivalent visual results on other window systems.
The diagnostic panes have application-owned content layout and no adaptive
scrolling. Some large-font content therefore overflows; it is not part of the
caption geometry check. Saved contained-window placement also remains durable
when zoom or host bounds change, with explicit bring-into-view recovery as
described in the original audit.
