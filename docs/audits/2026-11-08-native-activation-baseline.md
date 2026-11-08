# Native activation ordering baseline

This follow-up starts from `c0daedf` and checks focus when returning from a newly
created native child, subsequent keyboard window switching, and pane closure.
It fixes a delayed creation activation that stole focus back from the main
window after the child had already appeared ready.

## Reproduction and cause

The ordinary editor opens Inspector and Preview in a native child through its
toolbar. Returning to the main window then activates that child again. The
[baseline journey](2026-11-08-native-activation-baseline/baseline_window_focus.py)
failed its exact active-window assertion; its
[receipt](2026-11-08-native-activation-baseline/baseline-window.json) identifies
the unchanged starting binary. The
[before screenshot](2026-11-08-native-activation-baseline/first-return-before.png)
shows the child in front and the inactive main-window title bar.

Temporary targeted logs separated a delayed request from repeated activation or
a modal window rule. A diagnostic run left the visible child idle for more than
four seconds. Its global focus callbacks had all been recorded as Unknown,
because they arrived before the exact visible roster could prove their routes.
The first main-window activation then supplied a Known observation. Only at that
point did the old `TearOffCommitted` reservation emit its child `RequestFocus`.
Within the next sample, the child was active again. The
[timestamped observations](2026-11-08-native-activation-baseline/diagnostic-timing.json)
and [limited trace](2026-11-08-native-activation-baseline/diagnostic-activation.txt)
record this intermediate diagnostic build, not the final passing execution.

The code had equated an activation ordering baseline with a known current focus
owner. A legitimate Unknown envelope already supplies a provider generation.
It can order a requested platform effect without proving which window owns
focus. Waiting for Known unnecessarily delayed the request until unrelated new
user input supplied the missing fact.

## Change and boundaries

`activation_observation_baseline()` uses the existing observation stream's
generation when no tombstone is required. Explicit activation and the winning
native-create reservation can use that ordering baseline. When the current
owner is unknown and activation control is supported, they request native focus
immediately through the existing effect lane.

Known focus remains necessary for the already-focused shortcut and for actual
activation completion. Unknown does not become Known, publish a content-focus
success or substitute for a binding/effect acknowledgement. Duplicate baseline
observations cannot complete the new request. The original causal stamp,
binding validation, staging and first-live barriers remain intact.

No observations, missing envelopes, conflicting generations, generation gaps
and provider replacement still suppress activation until a valid ordered stream
exists. Observe-only input and lifecycle compensation retain their existing
Known-focus requirements. The change adds no state field, queue or public API.

## Regression checks

The coordinator regression requests activation from a valid Unknown baseline,
checks the exact pending generation and absence of fabricated focus, rejects
unsupported activation control, and proves that only a later exact Known
observation completes the request. A separate invalid-stream table covers the
rejected boundary states.

The existing hidden-window lifecycle regression now uses Unknown observations
through hidden, pre-show, visible, post-show and first-live admission. It still
requires the full presentation sequence and a real focus observation to finish
the emitted effect. The original implementation fails because the admitted
child has no exact focus request. The
[red](2026-11-08-native-activation-baseline/activation-red.log.gz) and
[green](2026-11-08-native-activation-baseline/activation-green.log.gz) runs retain
the executable comparison.

Four older fixture assertions assumed Unknown always suppressed focus effects.
They now require the exact new focus effect rather than discarding it: child
redocking preserves its unique release effect, concurrent admission permits only
the winning child request, and drag release still excludes pointer pass-through
restoration. Fixtures that continue running accept the request and report the
simulated platform observation through the public host interface. Their
[focused run](2026-11-08-native-activation-baseline/fixture-green.log.gz) passes.

Several native retirement and local-input models also relied on Unknown to
avoid requesting focus. Their setup now reports the exact already-active root
or input window through the formal API. Existing effect, byte-budget, edge-count
and quiescence assertions remain unchanged. The ordinary-focus pressure branch
still reports Foreign focus later and exercises its precise pending tokens.
All 25 tests in the four affected native modules pass, including the 10,000-cycle
reuse and controlled 1 kHz models; their
[raw log](2026-11-08-native-activation-baseline/native-fixtures.log.gz) preserves
the run. These are simulated scenario premises, not additional OS observations.

## Actual editor observations

The final ordinary editor used the existing Linux/Xvfb/Openbox/XTest software-GL
environment, with a 1000 × 780 main window and a 440 × 340 child. The
[bounded journey](2026-11-08-native-activation-baseline/window_close_journey.py)
uses actual mouse and key input. It asserts the exact active window and live
process at each recorded checkpoint, plus retention of the original child after
closing Inspector. Text, selected panes and focus rings were inspected in the
original screenshots; these are separate from the automated OS assertions.

| Journey | Observed result |
| --- | --- |
| First return after opening the child | The main window stays active; [after](2026-11-08-native-activation-baseline/first-return-after.png) |
| Four Alt+Tab switches after establishing each editor's focus | Text enters each window's original editor on both returns; [child](2026-11-08-native-activation-baseline/child-roundtrip-input.png) |
| Close Document and approve through the toolbar | History becomes selected. Toolbar confirmation does not force focus into its editor; [closed](2026-11-08-native-activation-baseline/document-closed.png) |
| Click History content after approval | History accepts `~HISTORY_ACTIVE~`, retained in the later native-successor screenshot |
| Close Inspector in the child and approve through the main toolbar | Preview remains in the same child, the main window stays active after approval, and Alt+Tab returns to that child; [successor](2026-11-08-native-activation-baseline/native-successor-return.png) |

The final journey's application stderr is empty. The
[run receipt](2026-11-08-native-activation-baseline/native-journey.json),
[checkpoint observations](2026-11-08-native-activation-baseline/window-observations.json)
and [source/binary identities](2026-11-08-native-activation-baseline/identity.json)
bind these results to the corrected production code. Subsequent changes update
test fixtures only. All temporary logging has been removed.

## Verification and review

All 11 serial gates exited successfully; the
[final receipts](2026-11-08-native-activation-baseline/verification.json) record
their commands and execution times. The focused regressions and actual-window
checks above have passed. Cargo uses one build job and nextest worker with the
existing target directories. The main result predates only native test-fixture
updates; its source scope has not changed afterward.

| Complete suite | Result | Raw log |
| --- | --- | --- |
| Main workspace, all features and targets | 2257 passed; 3 existing skips | [Log](2026-11-08-native-activation-baseline/workspace-all.log.gz) |
| Fork-backed native workspace, all features and targets | 615 passed; 1 existing skip | [Log](2026-11-08-native-activation-baseline/native-all.log.gz) |
| Native package, default features and all targets | 564 passed; 1 existing skip | [Log](2026-11-08-native-activation-baseline/native-default.log.gz) |
| Product integration harness, all features and targets | 119 passed | [Log](2026-11-08-native-activation-baseline/product-all.log.gz) |
| Host integration harness, all features and targets | 335 passed | [Log](2026-11-08-native-activation-baseline/host-all.log.gz) |

Main and native workspace Clippy checks passed with correctness and suspicious
lints denied. The Linux native workspace check passed with all features and
targets. Both workspace format checks and the diff check passed. Nonempty gate
logs are preserved as compressed files beside the receipts. Source hashes and
the final native binary still match the identities recorded for the actual
window journey.

Independent [Standards and Spec reviews](2026-11-08-native-activation-baseline/code-review.md)
found no remaining static issues in the production change or fixture updates.
These reviews did not execute tests or the GUI.

This audit qualifies the observed X11 software-GL behavior. It does not measure
docking performance or establish additional platform support. The existing
unrelated `docs/reviews/` directory is excluded.
