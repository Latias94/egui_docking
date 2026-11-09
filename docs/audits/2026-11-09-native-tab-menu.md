# Hidden-tab menu labels and native pointer isolation

This follow-up starts from `c33bc0f` and checks the actual egui hidden-tab menu
at ordinary and increased UI zoom. It found truncated titles and a native
runtime failure when clicking a menu row.

## Title clipping

Core already allocates menu padding and insets the row viewport. The egui
renderer added another eight logical points before painting each title, placing
the end of a longest title outside its clip. The correction removes that second
offset without changing row backgrounds or interaction geometry.

The owned-rendering regression opens the real menu through its AccessKit
button, verifies that both title shapes exist, and checks that each complete
galley fits its clip at zoom 1.0 and 1.5. AccessKit node bounds remain logical;
the test does not divide them by pixels-per-point again.
[Original failure](2026-11-09-native-tab-menu/title-red.log.gz).

## Pointer isolation

Clicking Assets in the actual native menu stopped the runtime with
`InputPrefixReductionFailed`. A temporary diagnostic exposed the exact cause:
the drag lane claimed the underlying `PaneBody`, while core's winner was
`TabListMenuBlocker`. The defect also reproduces at zoom 1.0 in the native
receiver regression, so zoom is not its cause.

The egui menu frame only sensed hover, and its backdrop only sensed clicks.
Core defines both regions across pointer lanes. The frame now senses click and
drag while remaining non-focusable; the backdrop adds drag sensing. Rows remain
click-only. The existing backdrop → frame → row registration order therefore
lets row clicks select a tab while the frame blocks the underlying drag target.
No resolver fallback, new receiver, core hit-rule change, scroll change or
window-activation workaround is needed.

The native regression measures, paints and presents the actual popup, then
sends native pointer press/release at an unselected row through the captured
egui hit authority, real receiver store and core reducer. It covers zoom 1.0 and
1.2, verifies both queried lanes, selection and menu dismissal, and checks the
captured frame-border and backdrop hits in both lanes.
[Red](2026-11-09-native-tab-menu/pointer-red.log.gz) and
[focused green](2026-11-09-native-tab-menu/pointer-green.log.gz). The focused
green predates the border/backdrop assertions; the final complete suites
include those assertions.

## Verification

The final ordinary-example journey checks readable Outline/Assets labels and
clicks Assets after increasing zoom, with an Inspector child also alive. The
resulting content and active selection are inspected in the original screenshot;
the journey script asserts exact window activation and a live process.

The [menu](2026-11-09-native-tab-menu/zoom-tab-menu.png) shows both complete
titles. The [selected Assets content](2026-11-09-native-tab-menu/zoom-menu-selected-assets.png)
confirms dismissal and selection with no stopped-runtime banner, compared with
the [original failure](2026-11-09-native-tab-menu/menu-before.png) and its
[diagnostic](2026-11-09-native-tab-menu/baseline-error.log.gz).
The [shared journey receipt](2026-11-09-native-retained-focus/final-journey.json)
and [source identities](2026-11-09-native-retained-focus/source-identity.json)
bind the final screenshots to the clean build; its stderr was empty.

The combined worktree's
[serial verification receipt](2026-11-09-native-retained-focus/verification.json)
covers product tests, native tests with default and all features, the product
integration harness, Clippy, formatting, the real Linux retention test and the
Linux example build. Core source is unchanged. Independent static review found
no unresolved issue in menu sizing, lane precedence or the real receiver seam.

The native row regression and actual-window journey establish X11 behavior.
This round does not claim native menu qualification on Wayland, macOS or
Windows, or changes to docking benchmark results.
