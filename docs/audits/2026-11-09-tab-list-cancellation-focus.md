# Tab-list cancellation keyboard focus

This follow-up starts from `1a69f37`. The ordinary native editor's hidden-tab
menu closed on Escape but left no keyboard focus. Enter could not reopen it;
Tab restarted at the main toolbar. The selected pane remained unchanged.

## Cause and correction

The menu's focused row requested egui focus on every encoding pass. Escape
queued its existing core dismissal without handing framework focus elsewhere.
After that dismissal committed, the row disappeared and egui cleared its dead
focus identity.

The menu trigger now derives its stable egui ID from the owning tab bar's
existing opaque visual identity. The popup exposes the same identity, so Escape
can return focus to the exact trigger. It does so only after preparing the
current dismissal and verifying that the button is registered, enabled, visible
and focusable in this pass. No widget-tree search, remembered focus cache, core
state or public API is added. Other tab-strip control IDs are unchanged.

An initial correction handled ordinary runs but failed when egui discarded and
repeated an encoding pass. The deferred dismissal was still pending, and the
menu row took focus back during the retry. The final change preserves trigger
focus on a repeated pass of that same run. The next run's first pass still
enters menu rows normally when reopening the popup. Pointer backdrop dismissal
and row activation retain their existing behavior.

## Regression

The new portable renderer test uses real egui runs, obtains the actual button
from AccessKit output, and opens the menu with its ordinary click action. It
then sends End and Escape. After deferred dismissal commits, the menu must be
absent, its owning button must hold framework focus, and the original pane must
remain selected. Enter must reopen the menu for another End/Escape cycle.
Application text remains unchanged; the test never requests focus directly.

The first Escape run uses the existing `LateDiscard` plugin to force three
actual passes and asserts their count. The second cancellation has no forced
discard. The original implementation fails with `focused: None`; the initial
correction also fails that same check under forced retries. The final
regression and the preceding menu-row activation/TextEdit regression pass.

The [original failure](2026-11-09-tab-list-cancellation-focus/menu-cancel-red.log.gz),
[initial correction's retry failure](2026-11-09-tab-list-cancellation-focus/menu-retry-red.log.gz)
and [retry correction's focused run](2026-11-09-tab-list-cancellation-focus/menu-retry-green.log.gz)
preserve those checks. The final full workspace run also passes both tests.

## Actual-window scope

The bounded Linux journey uses the ordinary native example, Xvfb/Openbox,
XTest input and software GL. It checks both the main window's menu and a
160 × 260 native child's menu. Menu visibility, pane selection and text are
inspected in original screenshots separately from automated window/process
assertions. Forced repeated passes are covered by the portable regression,
not by an assertion about how many passes the GUI happened to execute.

| Check | Original screenshot observation |
| --- | --- |
| Baseline main window: Escape, then Enter | Menu remains closed. [Before](2026-11-09-tab-list-cancellation-focus/main-reopen-before.png) |
| Final main window: Escape, then Enter | The original tab-list menu reopens. [After](2026-11-09-tab-list-cancellation-focus/main-reopen-after.png) |
| Final main window: End/Enter after reopening, then type | Assets is selected and accepts `~AFTER_CANCEL_REOPEN~`. [Input](2026-11-09-tab-list-cancellation-focus/main-input-after.png) |
| Final child window: Home/Escape, then Enter | The child's Preview/Inspector menu reopens. [Menu](2026-11-09-tab-list-cancellation-focus/child-reopen-after.png) |
| Final child window: End/Enter after reopening, then type | Inspector accepts `~CHILD_CANCEL_REOPEN~` without another content click. [Input](2026-11-09-tab-list-cancellation-focus/child-input-after.png) |

The [baseline journey](2026-11-09-tab-list-cancellation-focus/baseline_journey.py)
and [binary identity](2026-11-09-tab-list-cancellation-focus/baseline-identity.json)
identify the starting build. The
[final journey](2026-11-09-tab-list-cancellation-focus/final_journey.py),
[execution receipt and source hashes](2026-11-09-tab-list-cancellation-focus/final-journey.json)
and [final binary identity](2026-11-09-tab-list-cancellation-focus/final-identity.json)
identify the final run. Both scripts assert a live app and the exact active
window. Final application stderr is empty.

An earlier check on the unchanged starting binary also exercised Inspector
unload/reload in a native child: its original text survived, clicking Reload
allowed immediate input, and the button remained visible at 260 × 140. The
[earlier screenshot](2026-11-09-tab-list-cancellation-focus/earlier-child-reload.png)
and [identity](2026-11-09-tab-list-cancellation-focus/earlier-child-reload-identity.json)
record that separate check before the menu edits.

While preparing the child-menu check, attempts to open Inspector from a
680 × 620 main window were rejected with `surface 1 has no current presentation
geometry`. Expanding the host to 1000 × 780 before creation succeeded. This
separate observation is being investigated; the menu fix does not claim to
resolve it. The child-menu journey explicitly expands the host before creation.

## Verification and review

All [ten serial gates](2026-11-09-tab-list-cancellation-focus/verification.json)
passed using one Cargo job and nextest worker with the existing target paths:

| Suite | Result | Log |
| --- | --- | --- |
| Main workspace, all features and targets | 2258 passed, 3 existing skips | [Log](2026-11-09-tab-list-cancellation-focus/workspace-all.log.gz) |
| Native workspace, all features and targets | 616 passed, 1 existing skip | [Log](2026-11-09-tab-list-cancellation-focus/native-all.log.gz) |
| Native package, default features and all targets | 565 passed, 1 existing skip | [Log](2026-11-09-tab-list-cancellation-focus/native-default.log.gz) |
| Product integration harness, all features and targets | 119 passed | [Log](2026-11-09-tab-list-cancellation-focus/product-all.log.gz) |

Main and native workspace Clippy checks pass with correctness and suspicious
lints denied. Both format checks, the diff check and the Linux all-features
example build pass. The focused run predates only the equivalent relocation of
the retry check inside the focused-row branch; all final gates and the final
GUI use the final source arrangement.

Independent correctness and standards reviewers found no remaining substantive
issues. Correctness review identified the initial repeated-pass defect above;
the regression reproduced it before the final fix. Both reviewers then checked
the narrow retry guard and test behavior without running Cargo or the GUI.
Neither accessed the unrelated `docs/reviews/` directory.

This round does not measure docking performance or qualify Wayland, mixed DPI,
macOS or Windows behavior. Core source and the native host's protocol are
unchanged by this menu correction.
