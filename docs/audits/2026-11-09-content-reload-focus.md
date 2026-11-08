# Content reload keyboard focus

This follow-up starts from `68b70b9` and checks unloading, reloading and reopening
application-owned panes in the ordinary native egui editor. Reloading Document
preserved its text but left the restored editor without keyboard focus. Typing
immediately after the content area's Reload button therefore had no effect.

## Cause and correction

The placeholder's button called `set_loaded(true)` and requested repaint. The
button disappeared on the next frame without handing focus to the restored
TextEdit. The defect reproduces through `ExamplePanes::ui` without a docking
coordinator or native window, so the correction belongs to the example callback.

After an actual Reload button activation, the callback now queries the existing
`focus_target(item)` and requests that exact egui identity. The restored editor
registers it on the next frame. Preview, Console and Profiler still have no
editable focus target. Unknown panes still return before this branch.

The change adds three lines and no state, queue or public API. `set_loaded`
continues to preserve text without requesting focus itself. Inventory actions,
passive application reload and layout restoration therefore retain their
previous focus behavior. The unloaded placeholder still reports no editor
focus target.

## Regression

The new example test renders the real pane callback, sends Tab and then Enter
or Space to the Reload button, renders the restored editor and sends text. Both
activation paths must focus the exact editor ID, retain its original text,
accept the new text and leave Inspector's text unchanged. The test does not
request focus or insert text into application state directly.

The [original implementation fails](2026-11-09-content-reload-focus/reload-red.log.gz)
at `Enter on Reload content must focus the restored editor` after successful
button activation. The [corrected implementation passes](2026-11-09-content-reload-focus/reload-green.log.gz)
both keyboard paths.

## Actual-window observations

Both executions use the ordinary Linux example under Xvfb/Openbox, XTest input
and software GL, with a 1000 × 780 host window. Both scripts check that the app
is alive and record the active window, which matches the host in both receipts.
The final script also asserts that exact window match. Text and focus rings
were inspected separately in the original screenshots; the scripts do not
assert rendered text.

| Journey | Observed result |
| --- | --- |
| Baseline: edit Document, unload through inventory, click Reload, type without clicking the editor | Original `~RETAINED_CONTENT~` survives; `~AFTER_RELOAD~` is absent. [Before](2026-11-09-content-reload-focus/reload-before.png) |
| Corrected build: repeat that same journey | Both markers are present and the editor has its focus ring. [After](2026-11-09-content-reload-focus/reload-after.png) |
| Corrected build: close Document, approve, reopen from inventory and type without clicking its editor | The two earlier markers survive; `~AFTER_REOPEN~` is appended. [Reopened](2026-11-09-content-reload-focus/reopen-after.png) |

The [baseline script](2026-11-09-content-reload-focus/baseline_journey.py) and
[baseline identity](2026-11-09-content-reload-focus/baseline-identity.json)
identify the original `68b70b9` binary. The
[final script](2026-11-09-content-reload-focus/final_journey.py),
[per-run identity](2026-11-09-content-reload-focus/final-journey-identity.json)
and [source hashes](2026-11-09-content-reload-focus/identity.json) identify the
corrected execution. Source hashes were recorded after the journey; source did
not change during it. The final application stderr was empty. Keyboard
Enter/Space coverage comes from the egui regression, not these mouse journeys.

## Verification and review

All [six serial verification steps](2026-11-09-content-reload-focus/verification.json)
passed with one Cargo build job and one nextest worker, reusing the existing
target directories:

- Native workspace, all features and targets: 616 passed, 1 existing skip.
  [Log](2026-11-09-content-reload-focus/native-all.log.gz)
- Native package, default features and all targets: 565 passed, 1 existing skip.
  [Log](2026-11-09-content-reload-focus/native-default.log.gz)
- Native package Clippy, all features and targets, with correctness and
  suspicious lints denied. [Log](2026-11-09-content-reload-focus/native-clippy.log.gz)
- Native workspace format check and `git diff --check`.
- Linux all-features example build before the final window journey.
  [Log](2026-11-09-content-reload-focus/linux-build.log.gz)

Independent correctness and standards agents reviewed the two source files
against `68b70b9` and found no substantive issues. They checked that the request
is limited to explicit button activation, the target can register on the next
egui pass, read-only panes remain excluded, and the regression exercises actual
keyboard and TextEdit behavior. Both reviews were static; they ran no tests or
GUI. Neither accessed the unrelated existing `docs/reviews/` directory.

Core and renderer source did not change, so their preceding full-suite results
were not rerun for this example-only fix. This round does not measure docking
performance or qualify child-window, Wayland, macOS or Windows reload behavior.
