# Overflow menu keyboard focus

This follow-up starts from `000c484` and checks keyboard access to pane discovery
and hidden tabs in the actual egui editor. The ordinary pane inventory already
supports keyboard navigation and selection. The hidden-tab menu exposed a focus
handoff defect: End followed by Enter selected Assets, but typing immediately
afterward never reached its editable content.

## Cause and correction

Menu selection and menu closure already committed together. The shared reducer
requested native activation when a live native binding existed, but had no local
content-focus fallback. Adding that fallback fixed the portable renderer test,
yet the actual native editor still lost input.

The native diagnostic explained the difference. The current window received
keyboard input while desktop-wide focus authority was unavailable. Activation
returned `ObserveOnlyRecorded` with `observation_baseline: None`, leaving no
pending pane intent. A current-UI keyboard action therefore waited for a global
observation it did not need.

The reducer now takes a private `TabListMenuActivation` enum:

- Local semantic, accessibility and prepared activation requests content focus
  through the existing local pane-intent path, preserving the input's causal
  stamp and the current native binding guard.
- Real pointer journal activation keeps the native ordered-activation path. Its
  pending platform effects and delayed activation compensation remain intact.
  Without an activation-capable native binding it uses the local fallback.

Both paths use `finish_tab_list_menu_row` for the existing menu validation,
selection and closure. Selecting an already selected row also requests content
focus. The change adds no public API, command queue or focus observation. The
host still confirms actual focus through its existing reporting path.

## Regression evidence

The renderer regression uses two real TextEdit panes and an overflowing tab bar.
It opens the actual menu, selects the second pane with End/Enter, reopens it and
reselects that pane with End/Space. After deferred selection settles, it checks
the exact egui focus ID and sends text. Both text inputs reach the second pane;
the first remains empty. The original reducer fails with `focused: None`.

The core regression retains the exact workspace-event cause assertion and adds
the pending pane target, surface and native guard. It runs without native
registration and with a visible, activation-capable native root that has no
global focus baseline. Observed focus must remain unchanged.

A controlled reversal routed Local through the old native-observation branch
while retaining the final fixture. The native case then failed because no pane
intent was created; restoring the corrected route passed. The
[controlled diff](2026-11-08-overflow-keyboard-focus/native-menu-focus-red.diff),
[failure](2026-11-08-overflow-keyboard-focus/native-route-red.log.gz) and
[passing run](2026-11-08-overflow-keyboard-focus/native-route-green.log) preserve
that check. The separate
[renderer failure](2026-11-08-overflow-keyboard-focus/renderer-red.log.gz) records
the original symptom.

## Actual-window checks

The final Linux editor was rebuilt from the corrected sources and exercised at
680 × 620 with XTest input under Xvfb/Openbox and software GL. The bounded
[input journey](2026-11-08-overflow-keyboard-focus/overflow_keyboard_journey.py)
asserts the original host identity, active window and live application at the
end of the journey. Text and widget state were inspected in the original
screenshots, separately from those OS assertions.

| Check | Observed result |
| --- | --- |
| Baseline End/Enter selects Assets, then types `~ASSETS_AFTER~` | Assets is visible but contains no marker; [before](2026-11-08-overflow-keyboard-focus/assets-input-before.png) |
| Final End/Enter selects Assets, then types without clicking its content | `~ASSETS_AFTER~` appears in Assets |
| Final End/Space reselects Assets, then types without clicking its content | Both Assets markers remain visible with the content focus ring; [after](2026-11-08-overflow-keyboard-focus/assets-input-after.png) |
| Final Home/Enter returns to Outline, then types without clicking its content | The earlier Outline marker survives and the new marker is appended; [returned pane](2026-11-08-overflow-keyboard-focus/outline-input-after.png) |
| Baseline inventory opened and navigated with Tab, Enter and arrow keys | Inspector selection completes; [action feedback](2026-11-08-overflow-keyboard-focus/inventory-keyboard-action.png) |

The before screenshots and inventory journey were captured before this round's
edits using the preceding editor build. That build's identity is recorded by the
[previous audit](2026-11-08-native-recovery/environment.json); these two baseline
journeys did not capture a separate per-run binary checksum. The inventory
journey ran on the baseline only. No inventory behavior is changed here.
The final overflow journey has an empty application stderr. Its
[execution receipt](2026-11-08-overflow-keyboard-focus/native-journey.json),
[window state](2026-11-08-overflow-keyboard-focus/window-state.json) and
[source/binary identities](2026-11-08-overflow-keyboard-focus/identity.json)
identify the final execution. The diagnostic build's limited
[activation trace](2026-11-08-overflow-keyboard-focus/native-observation-diagnostic.txt)
is cause evidence, not a final passing run. All temporary probes were removed.

## Verification and review

All 11 serial gates exited successfully; their commands and execution times are
recorded in the [final receipts](2026-11-08-overflow-keyboard-focus/verification.json).
Cargo uses one build job and nextest worker, with the existing target directories.
The separate Linux example build succeeded before the final window journey.

| Test suite | Result | Raw log |
| --- | --- | --- |
| Main workspace, all features and targets | 2255 passed; 3 existing skips | [Log](2026-11-08-overflow-keyboard-focus/workspace-all.log.gz) |
| Fork-backed native workspace, all features and targets | 615 passed; 1 existing skip | [Log](2026-11-08-overflow-keyboard-focus/native-all.log.gz) |
| Native package, default features and all targets | 564 passed; 1 existing skip | [Log](2026-11-08-overflow-keyboard-focus/native-default.log.gz) |
| Product integration harness, all features and targets | 119 passed | [Log](2026-11-08-overflow-keyboard-focus/product-all.log.gz) |
| Host integration harness, all features and targets | 335 passed | [Log](2026-11-08-overflow-keyboard-focus/host-all.log.gz) |

Main and native workspace Clippy checks passed with correctness and suspicious
lints denied. The Linux native workspace check passed with all features and
targets. Both workspace format checks and the diff check passed. Final staged
source hashes and the Linux binary checksum still match the window journey's
recorded identities. The evidence directory includes compressed nonempty gate
logs and the separate example build log.

Independent [Standards and Spec reviews](2026-11-08-overflow-keyboard-focus/code-review.md)
confirmed the local/journal split and test scope. Both identified a conditional
import error, which was corrected before the successful production build. Their
final static reviews have no remaining findings.

This audit does not measure docking performance or qualify additional desktop
platforms. The unrelated existing `docs/reviews/` directory is excluded.
