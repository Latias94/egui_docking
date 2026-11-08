# Native inspection and pane workflow

This follow-up implements the
[native inspection and pane workflow plan](../plans/2026-11-08-native-inspection-and-pane-workflow.md)
from `7a260d28b693e886b7cf3020cd807fbf7ed1a965`. The changes simplify a misleading
inspection contract and correct two concrete editor workflows.

| Before | After | Why |
| --- | --- | --- |
| `with_view` always returned `Some`, but every caller handled an absent workspace. | It returns the callback's result directly. The private runtime exposes its committed view without an optional callback wrapper. | Native shutdown retains the committed session; those absence branches described no possible state. |
| Reopen looked for the original `SURFACE` when Document was closed. | Reopen uses the current `root_surface()` and the current main area. | Rebinding changes the logical host identity while retaining the application window. |
| Any contained pane on the host had its Float action disabled. | A pane can leave a multi-pane contained group. A standalone floating pane retains its separate raise/recovery controls. | The previous condition hid an existing supported operation. |
| Reopen and Move repeated main-area destination selection and scanned the pane inventory. | Both use one example-owned topology query with central-area preference and an item fallback. | Application destination policy stays local, with no copied registry or new core command framework. |
| Successful pane movement fell through to debug-formatted core outcomes. | Feedback names panes and explains docking, floating, raising, recovery and unchanged results. | Users can see what happened without interpreting internal identities. |

## Inspection migration

The public inspection change is intentionally breaking. Remove only the outer
optional-view handling; item and other domain lookups inside the callback can
still return `None`:

```rust,ignore
let host = app.root_surface();
let open_on_host = app.with_view(|view| {
    view.item(item).is_some_and(|pane| pane.surface() == host)
});
```

The callback always runs, before native attachment and after fatal shutdown,
while holding the same runtime lock. Other runtime calls belong outside that
callback. All repository consumers are migrated, including Linux-only native
tests, physical-input observers, presentation continuity and output-pressure
fixtures. The [migration guide](../ENGINE_FIRST_MIGRATION.md#egui-integration)
documents the final contract.

## Verification

The two workflow regressions failed before correction. The retained
[red receipt](2026-11-08-pane-workflow/workflow-red.json) and
[log](2026-11-08-pane-workflow/workflow-red-final.log.gz) show the stale-host
reopen failure and the disabled Float button. Four focused editor tests now
verify current-host destinations for central, non-central and valid rootless
layouts, search beyond an excluded item in a split, activate the real Float
button with AccessKit, and retain singleton group-recovery controls.

Final serial verification passed with one Cargo build job and one nextest worker:

| Gate | Result |
| --- | --- |
| Fork-backed native workspace, all features and targets | 610 passed; 1 existing skip |
| Native package, default features and all targets | 559 passed; 1 existing skip |
| Native public-command doctest | 1 passed |
| Native workspace correctness/suspicious Clippy and cargo fmt | Passed |
| Linux example build, all features | Passed |
| Linux workspace check, all features and targets | Passed |
| Nine-command actual-window journey | Passed; presented and quiescent |

The Linux check also found a test-compilation regression left by the preceding
command-interface refactor: the X11 boundary test depended on `ItemId`, `RootId`
and `NativeWindowPlacement` imported incidentally by its parent module. Those
dependencies are now imported directly in the test. The
[original compiler output](2026-11-08-pane-workflow/linux-consumers-check.log.gz)
and final passing check are retained.

The existing nine-command smoke now starts with Editor and Inspector together
in one contained root. Its first applied Float result must leave Inspector in
that original root and Editor in a separate singleton. The journey then raises,
recovers and docks content, completes both native-child return paths, retains all
eight panes and their text, and ends with one presented, quiescent host. The
[receipt](2026-11-08-pane-workflow/actual-command-smoke.json) and
[exact outcomes](2026-11-08-pane-workflow/actual-command-smoke.stderr) record the
successful 7.21-second functional journey, not a performance benchmark.

A separate ordinary editor window received actual X11 mouse clicks through
**Pane inventory → Inspector → Move → Float in main window**. The screenshots
show the enabled command and the resulting independent Inspector, with Preview
remaining in its original group and the new readable success feedback:

| Observation | Evidence |
| --- | --- |
| Inspector and Preview before extraction | [Before](2026-11-08-pane-workflow/before.png) |
| Inventory and enabled Float command | [Menu](2026-11-08-pane-workflow/move-menu.png) |
| Independent Inspector, retained Preview and success feedback | [After](2026-11-08-pane-workflow/after-float.png) |

Both window checks used the existing one-CPU, 4 GiB Linux/Xvfb/Openbox software-GL
environment. They do not qualify physical dragging, additional platforms or a
hardware GPU. The [review summary](2026-11-08-pane-workflow/code-review.md)
records independent Standards and Spec reviews and the closed evidence finding.

[Verification receipts](2026-11-08-pane-workflow/verification.json),
[environment and binary identity](2026-11-08-pane-workflow/environment.json), and
[changed Rust source hashes](2026-11-08-pane-workflow/tested-sources.json) identify
the tested source and the timing of the final Linux-only test-import correction.
Shared core/portable adapter Rust sources were unchanged; their complete suites
were not rerun. Remote CI was not executed during local verification.
