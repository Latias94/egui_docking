# Native close confirmations and focus

This implements the [close confirmations and focus plan](../plans/2026-11-08-native-close-confirmations.md)
from `e9887c9c215a58ae745286b5a8f56d982008aa06`. Actual editor input exposed two
baseline focus defects while checking the close workflow.

| Before | After |
| --- | --- |
| Inventory selection reported that Inspector was already selected while its existing native window remained behind the host. | Explicit product selection uses the existing native activation path, including repeated selection. |
| Home/End initially focused the destination tab, but deferred selection later moved focus into editable content. | Navigation retains its focus purpose through direct and deferred submission; Enter and explicit activation still enter pane content. |
| Close controls exposed request IDs, target debug output, Allow/Veto/Defer and continuation terminology. | Named confirmations use Close, Keep open and Decide later, with the same Close/Keep open controls after postponement. |

## Focus corrections

Explicit product selection previously called `request_local_pane_focus`, the
observe-only path used by pointer tab gestures. It could request editable content
focus but could not activate an existing native window. Selection now uses
`start_viewport_activation` when a current native focus binding exists. Eligibility,
platform capability, causal ordering and observation acknowledgement remain with
the existing coordinator. Without a native binding, selection retains its local
pane-focus behavior. Pointer gestures, movement and close recovery retain their
existing rules.

The [core red test](2026-11-08-close-focus/focus-red.log.gz) demonstrates the
missing exact-binding `RequestFocus`. Its regression selects an already-selected
pane in another native window, verifies that native observation is still
required, and checks that the subsequent pane intent does not claim observed
content focus.

Keyboard navigation used to prepare an ordinary tab activation, losing the
distinction between selecting a tab for continued navigation and entering its
content. A crate-private `LocalTabFocus` now travels inside the existing opaque
action. Both purposes share frozen-witness validation, selection, contained
raising and submission. Navigation preserves framework focus and advances the
existing coordinator's causal claim; this retires older unfinished pane intents
and suppresses late older native activation. It creates no content-focus request
and changes no observed-focus history.

The renderer regression now uses real `TextEdit` targets. It checks End, Home,
Right and Left during the immediate pass, after queued selection commits and in
an additional idle pass. It replaces the weaker regression whose panes exposed
no editable focus target and whose focus assertion preceded submission. The
initial activation is settled before navigation begins. With this same final
fixture, restoring only the old navigation-to-content behavior fails on End's
idle pass. The [controlled one-line diff](2026-11-08-close-focus/keyboard-red.diff),
[red receipt](2026-11-08-close-focus/keyboard-red.json),
[red log](2026-11-08-close-focus/keyboard-red.log.gz) and
[passing candidate log](2026-11-08-close-focus/keyboard-final-green.log.gz)
record the comparison. A separate coordinator regression checks retirement of
old requests without fabricating observed focus.

## Close interaction

Pending and postponed decisions render through one submission loop. The example
continues to use exact core-issued initial and deferred tokens and the existing
bounded command/result slot. Accepted commands disable the remaining controls
until their status is consumed. Accessible names identify both pane and action,
such as `Inspector: Keep open`. A group explicitly explains that all panes close
together and that keeping any member open cancels the complete request.

Two controls tests use real session close plans, activate actual AccessKit
buttons, check slot blocking and verify that postponed decisions offer no second
postpone action. The ambiguity regression failed before the UI correction;
its [receipt](2026-11-08-close-focus/confirmation-red.json) and
[log](2026-11-08-close-focus/confirmation-red.log.gz) are retained.

## Actual-window evidence

Both final journeys used the same rebuilt example in the existing one-CPU,
4 GiB Linux/Xvfb/Openbox software-GL environment. Ordinary mouse/key input drove
the editor; no private runtime command injection was used.

| Observation | Evidence |
| --- | --- |
| Before correction, selection left the existing native child behind the host. | [Before](2026-11-08-close-focus/native-select-before.png) |
| Selecting Inspector activates its existing child; typed text enters Inspector. | [After](2026-11-08-close-focus/native-select-after.png) |
| Group confirmation names both panes and explains atomic closure. | [Confirmation](2026-11-08-close-focus/close-confirmation.png) |
| Inspector can postpone while Preview is ready to close. | [Postponed](2026-11-08-close-focus/close-postponed.png), [partly confirmed](2026-11-08-close-focus/close-partly-confirmed.png) |
| Keep open cancels the complete group request and preserves the exact child. | [Cancelled](2026-11-08-close-focus/close-cancelled.png) |
| A second request closes both members, then Inspector reopens with the entered marker retained. | [Closed](2026-11-08-close-focus/group-closed.png), [reopened](2026-11-08-close-focus/inspector-reopened.png) |
| Before correction, End selected Inspector but moved focus into its editor. | [Before](2026-11-08-close-focus/keyboard-before-content-focus.png) |
| Repeated Home/End/Right/Left navigation retains tab focus; Enter then enters editable content. | [Tab focus](2026-11-08-close-focus/keyboard-after-tab-focus.png), [next Home](2026-11-08-close-focus/keyboard-repeat-home.png), [Enter](2026-11-08-close-focus/keyboard-enter-content.png) |

The [close/focus journey](2026-11-08-close-focus/close_focus_journey.py)
asserts the active X11 window, retention of the exact child after cancellation,
and absence of a visible child after approval. Its
[receipt](2026-11-08-close-focus/close-focus-window-run.json) records execution.
The [keyboard journey](2026-11-08-close-focus/keyboard_navigation.py) captures
Home/End/Home/Right/Left/End and Enter; its
[receipt](2026-11-08-close-focus/keyboard-window-run.json) does not by itself
assert widget state. Destinations, focus rings, decision states and retained text
were checked in the original full-window screenshots; those
[visual observations](2026-11-08-close-focus/visual-observations.json) are
separate from automated OS assertions.

These checks qualify this X11 software-GL environment. They do not establish
hardware-GPU behavior, other platforms or performance measurements.

## Verification and review

Final [serial verification receipts](2026-11-08-close-focus/verification.json)
and independent review are stored alongside the evidence. The
[environment](2026-11-08-close-focus/environment.json) identifies
the Linux binary; [source hashes](2026-11-08-close-focus/tested-sources.json)
identify the changed Rust sources.

| Gate | Result |
| --- | --- |
| Main workspace, all features and targets | 2254 passed; 3 existing skips |
| Fork-backed native workspace, all features and targets | 612 passed; 1 existing skip |
| Native package, default features and all targets | 561 passed; 1 existing skip |
| Main workspace doctests / native public-command doctest | 11 / 1 passed |
| Official egui public consumer | 24 passed |
| Product consumer, default / persistence | 114 / 119 passed |
| Independent host consumer, default / persistence | 292 / 335 passed |
| Main and native correctness/suspicious Clippy, strict docs and formatting | Passed |
| Linux native build and all-feature/all-target consumer check | Passed |
| Final close/focus and keyboard actual-window journeys | Passed OS assertions and screenshot inspection |

The [independent review](2026-11-08-close-focus/code-review.md) records Standards
and Spec conclusions separately, including the corrected evidence-reference
finding. Cargo used one build job, nextest one test worker, and all Cargo checks
ran sequentially with the existing target directories.

No public command or focus-policy type was added. Application data ownership,
exact close decisions and the single bounded command/result slot remain the
integration contract. Remote CI and publication are outside this local check.
