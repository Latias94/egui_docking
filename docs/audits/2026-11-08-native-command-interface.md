# Native application command interface

Native menus and command palettes now submit ordinary docking intents through
`NativeDockspaceApp::request_command(NativeCommand)`. The change removes ten
per-operation public methods and their private forwarding methods, exposes four
existing core operations, and completes their workflow in the native editor.
It implements the [approved plan](../plans/2026-11-08-1016-refactor-native-command-interface-plan.md)
from base commit `7627acb9aa382e609683588e15f54f6279b7cc6a`.

## Interface and ownership

`NativeCommand` is a non-exhaustive, copyable intent value with fourteen variants.
Applications can retain it in menus without retaining a prepared action:

```rust,ignore
let command = NativeCommand::FloatItem { item, surface };
app.request_command(command)?;
// On a later update, consume the terminal result before the next command.
if let Some(result) = app.take_action_status() {
    handle_command_result(result);
}
```

Admission prepares the intent under the existing runtime lock. The existing
single action/result slot continues to own ordered execution, revision checks,
Busy/Stopped admission and native presentation settlement. Queue acceptance is
separate from command success; a terminal result keeps the slot Busy until the
application consumes it. Layout and document replacement retain their dedicated
preparation and root-rebinding interfaces. Pane data remains application-owned.

The newly reachable operations are item docking, item floating on an existing
surface, raising a contained group, and explicitly moving that group into current
visible bounds. Raising preserves pane selection; recovery preserves stacking.
Passive painting does not change durable bounds. The editor's pane inventory
offers these operations through its context-sensitive **Move** submenu and
reuses its pending-action feedback. The
[migration guide](../ENGINE_FIRST_MIGRATION.md#egui-integration) describes the
breaking interface and result-consumption contract.

## Correctness repairs

The fixed application host must survive cross-surface movement as well as close.
A command that would remove its sole presentation now returns
`HostSurfaceRequired` without occupying the action slot. Central areas, companion
presentations and same-surface moves preserve the existing behavior. Missing
targets still reach core rejection, and a locked move retains its committed
`PolicyDenied` result. A regression initially failed when the host guard took
precedence over that locked policy result; the corrected guard leaves it to core.

The actual-window journey exposed a second defect after a singleton moved to a
native child. Returning that item produced a valid `Docked` or `Floated` outcome,
but the native terminal correlator only accepted complete-root outcomes and
stopped with `ApplicationActionOutcomeMissing`. It now accepts both exact
singleton outcomes while preserving transition identity, source/target matching
and the original item result. The
[failing journey](2026-11-08-native-commands/actual-command-smoke.stderr) and
[successful journey](2026-11-08-native-commands/actual-command-smoke-2.stderr)
retain the observed regression and correction.

## Verification

Cargo verification ran serially with one build job, one nextest worker and reused
targets. Final receipts and compressed raw logs are in
[verification.json](2026-11-08-native-commands/verification.json).

| Gate | Result |
| --- | --- |
| Fork-backed native workspace, all features and targets | 606 passed; 1 existing skip |
| Native package, default features and all targets | 555 passed; 1 existing skip |
| Native public-command doctest | 1 passed |
| Native workspace correctness/suspicious Clippy | Passed |
| Native workspace cargo fmt check | Passed |
| Linux native example build, all features | Passed |
| Nine-command actual-window journey | Passed; final state presented and quiescent |

Coordinator tests cover typed outcomes, pane identity/data, Busy until result
consumption, missing targets and geometry, no-op raise/recovery, locked moves,
stale preparation and fixed-host departure. Existing close, defer/veto, shutdown,
layout and root-rebinding tests now use the same public command representation.
No removed ordinary request method remains in live native or smoke Rust sources.
The [independent review summary](2026-11-08-native-commands/code-review.md)
records the closed policy finding and the final settlement review.

The finite `basic --command-smoke` journey performs Float, Raise, Bring Into View
and Dock, then makes two singleton native-child round trips, returning once by
item floating and once by item docking. All nine exact terminal results are
consumed; eight stable panes and their document text survive. It waits for fresh
presentation between operations and ends with only the application host.
CI runs the same mode with a bounded timeout and checks its completion marker.

The [actual-window receipt](2026-11-08-native-commands/actual-command-smoke-2.json)
completed successfully in 3.81 seconds in the existing one-CPU, 4 GiB
Linux/Xvfb/Openbox software-GL environment. This duration describes a functional
journey, not a docking benchmark. The check exercises public programmatic
commands in actual native windows; physical dragging, hardware GPU behavior and
additional platforms were not qualified by this run.

[Environment and binary identity](2026-11-08-native-commands/environment.json)
and [changed Rust source hashes](2026-11-08-native-commands/tested-sources.json)
bind the evidence to the base commit plus this implementation. The doctest
predates only the final two settlement match arms; its public example is
unchanged. Core docking algorithms and portable adapter Rust sources were
unchanged, so their full workspaces were not rerun. Remote CI was not run during
this local verification.
