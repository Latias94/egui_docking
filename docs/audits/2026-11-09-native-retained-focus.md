# Editor focus across retained native output

This follow-up starts from `c33bc0f`. In the ordinary native editor, minimizing
and restoring the main window could leave Document unable to accept text until
clicked again. Repeated cycles also exposed the same symptom in Inspector's
child window.

## Cause and correction

The first captured loss occurs in a root retain pass: egui still holds the
Document ID, but current presentation authority and its focus bindings are
unavailable. No pane callback registers that ID, so egui's end-of-pass deadman
switch clears it. Core may also retire the old receiver artifact during the
presentation transition; consulting that store alone cannot preserve liveness.

A second captured path bypasses the ordinary surface driver: a deferred child
directly retains its output and paints a placeholder. Budget-related early
returns likewise skipped the existing focus-retention call. Repairing only the
ordinary retain branch passed the isolated regression but still lost text in
repeated actual-window checks.

All three entry shapes now preserve the existing selected pane's stable focus
ID while skipping its UI. The common selection traversal is shared with focus
sampling. The native adapter verifies viewport, window, create attempt, surface
and exact output binding. Published-view retention additionally requires a
current session binding; candidate retention excludes workspace replacement.
An unpublished update continues to use the published model until commit.

This only registers an already focused state ID with egui. It neither requests
focus nor registers a widget or hit receiver. Closed, unselected, relocated panes
and panes withdrawing their focus target no longer match. Presentation-qualified
focus sampling keeps its existing requirements; liveness without a current
witness remains `Unknown`.
No production cache, queue, new core protocol or public product API is added.

## Regressions and actual windows

- The managed receiver regression paints and presents a surface, retires its
  old receiver artifact through a minimized snapshot, restores the window facts,
  skips two real egui passes, and then delivers text to the returning TextEdit.
  It checks both absent widget registration and `Unknown` focus sampling.
  [Red](2026-11-09-native-retained-focus/focus-red.log.gz) and
  [green](2026-11-09-native-retained-focus/focus-green.log.gz).
- The Linux eframe regression uses real output tokens and the production
  finalizer to exercise the shared direct-retention entry. It skips two passes
  without pane callbacks or editor widgets, then verifies focus and resumed
  rendering. It is an entry regression using the root token, rather than a
  simulated child lifecycle.
  [Red](2026-11-09-native-retained-focus/entry-red.log.gz) and
  [final Linux check](2026-11-09-native-retained-focus/linux-retain.log.gz).
- The product regression checks blurred-window liveness, deselection, a changed
  target and the absence of focus resurrection.

The final ordinary-example journey uses Xvfb/Openbox, XTest input and software
GL. It minimizes each real window, verifies the hidden state, restores it,
checks the exact active window and types without clicking its content. Text
and focus rings are inspected in the original screenshots; the script asserts
window state and process liveness rather than OCR text. The final journey and
source/binary identity are retained with the evidence.

The final clean build completed six cycles in each window. All six appended
markers and the editor focus rings are visible in the
[Document](2026-11-09-native-retained-focus/host-restored-5.png) and
[Inspector](2026-11-09-native-retained-focus/child-restored-5.png) screenshots.
Clicks and typing after zoom also worked in
[Document](2026-11-09-native-retained-focus/zoom-host-input.png) and
[Inspector](2026-11-09-native-retained-focus/zoom-child-input.png).
The [script](2026-11-09-native-retained-focus/final_journey.py),
[execution receipt](2026-11-09-native-retained-focus/final-journey.json),
[window observations](2026-11-09-native-retained-focus/observations.json) and
[source identities](2026-11-09-native-retained-focus/source-identity.json)
identify this run. Final application stderr was empty.

## Verification and limits

The [serial verification receipt](2026-11-09-native-retained-focus/verification.json)
records the exact tests, lint checks, format checks and Linux build. Cargo uses
one build job, nextest uses one worker, and existing targets are reused. This
worktree also contains the separately documented tab-menu correction.

| Check | Result |
| --- | --- |
| egui package, all features and targets | 142 passed |
| Native workspace, all features and targets | 618 passed, 1 existing skip |
| Native package, default features and all targets | 567 passed, 1 existing skip |
| Product integration harness | 119 passed |
| Real Linux eframe retention entry | 1 passed |
| Both Clippy correctness/suspicious gates, both format gates, diff check and Linux build | Passed |

Non-blocking lint warnings remain; the Clippy gates deny correctness and
suspicious lints rather than every warning. Core and the host-conformance
harness are unchanged, so their preceding complete results were not rerun.

Independent correctness and standards reviews checked native identity and epoch
boundaries, all retained-output entries, pane selection and the distinction
between liveness and presentation evidence. No unresolved findings remained.
Reviewers ran no tests or GUI and did not access the unrelated `docs/reviews/`.
This round does not qualify macOS, Windows or Wayland window restoration, and
does not change or remeasure the docking algorithm's performance.
