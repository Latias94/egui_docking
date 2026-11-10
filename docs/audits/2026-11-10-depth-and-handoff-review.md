# Deep-layout and endpoint-handoff review

Date: 2026-11-10 (Asia/Shanghai)

Baseline: `1365536`. This follow-up rotates three independent review lanes across
core presentation/capacity, egui input and native application commands. The
primary agent reproduces defects, integrates repairs and runs Cargo serially
with one build job and the existing target directories. The excluded review and
reference directories remain untouched.

Four P2 defects are confirmed. No new P1 is established. Implementation and
independent code review are complete. All fifteen serial gates and actual
Linux X11 qualification pass against implementation `b6fc6e5`.

## D01: native captions overflow the call stack on a supported deep layout

The core accepts a 10,000-level alternating split layout, but native window
caption construction recursively visited every split. Declaring or refreshing
such a viewport could abort the process. An isolated subprocess reproduced the
stack overflow before repair in nextest run
`9a8e4b9e-1cd6-486a-be7e-985842eaa82c`.

[window_title.rs](../../crates/egui_dockspace_native/src/window_title.rs) now uses
an explicit traversal stack. Reversing only newly appended children preserves
the original depth-first, left-to-right leaf order. The public layout regression
checks the entire caption, including all 10,001 selected leaves and fallback
titles. The targeted three-test caption group passes in
`00adb4e1-968b-481b-83e2-54829cae714d`.

## D02: debugging a public view recursively expands the same deep tree

The owned declaration already reports split child counts, but the borrowed
`DockspaceSplitView` formatter collected child views and recursively formatted
them. Formatting the workspace, surface, root, node or split could therefore
abort on that supported layout. The public host subprocess reproduces the
stack overflow in `2f7a508e-ef4c-4563-9d8a-6957410dd1ae`.

[view.rs](../../crates/dockspace/src/model/view.rs) reports `axis`, `child_count`
and immediate-child `weights` without expanding descendants. The public
traversal API is retained. The depth regression checks all five entry points;
its bounded-output assertion applies to the tested binary chain, not to an
arbitrarily wide split's weight list. All four deep-layout host tests pass in
`c7071292-96e2-49ef-8296-eace4f6368e5`, including full measurement and encoding.

## D03: a full stream budget prevents a drained endpoint from handing off

A managed public host can paint a headless surface, settle its output and then
register the same surface as a native root. At a full stream limit, the new
endpoint needs another stream, while the active headless stream can become
retiring only after that new emission. Repeated zero-output publications cannot
break the cycle. The one-slot public regression fails with `OutputCapacity` in
`e6551f7f-792c-4039-87ff-fce7d83090d8`.

The capacity model now reserves one global stream slot for endpoint handoff.
With an ordinary limit of N, outstanding outputs remain bounded by N and stream
metadata by N+1. A new unrelated surface cannot use the extra slot. The old
stream must be the same host's directly preceding endpoint for that surface,
with every pending output settled in the exact accepted candidate. Merely
receiving terminal reports does not grant this credit.

The core returns a typed admission fact before encoding. The existing per-pass
surface reservation now retains that fact, including its exact predecessor;
restart clears it and suspend/resume preserves it. Final preparation rechecks
the candidate ledger's `EndpointSuperseded` lifecycle, endpoint change, active
successor and drained predecessor. Both admission and final preparation use the
same capacity rule. Installation still occurs only after successful core
publication. The existing retirement, authority and exact quiescence rules
remain the reclamation mechanism.

The bounded profile prepays N+1 stream backing and stream-sized preparation,
capture and quiescence scratch. N+1 uses checked arithmetic. An unchanged
capacity configuration accepts its already authorized overlap; reducing a
limit cannot reinterpret ordinary history as a handoff. The minimum byte
allowance for `NativeReportStorageLimit` increases to cover this transition
storage; a formerly exact small allowance may now need adjustment.

The public regressions cover pre-settled and same-candidate settlement,
in-flight rejection, abandoned and failed commits, old receiver rejection,
two surfaces competing in either order, unrelated surfaces, repeated queries,
existing-stream redraws, exact terminal results and the unchanged output and
byte limits. The first eight-test host subset passes in
`8846754f-db0f-4234-8e22-f46d3f0ab696`; final full-host results are recorded below.
The first full-host run passed 342 tests and exposed an older test that required
raising the stream limit from one to two to escape the wait. That test now
requires fixed-capacity handoff, accepts same-value configuration during the
authorized overlap and retains its zero-callback, zero-publication assertions
while the native output is still in flight. Its initial transcript is retained.

## D04: a retried egui pass reopens a touch menu after Escape

The owned-run path staged a previous long-press request on every actual pass.
The first pass could open the presentation menu and then consume Escape to
close it. A discarded pass repeated the open request after egui had exhausted
the original key event. The menu consequently remained open in later idle
frames. The public owned-run regression reproduces this after two late
discards in `ed5bd17e-d093-40a9-be3b-8ccc1d764856`.

[product_owned_run.rs](../../crates/egui_dockspace/src/product_owned_run.rs) now
stages that input intent only on the run's existing first-pass boundary. Later
passes preserve egui's popup state, including cancellation. The original queued
request is acknowledged only after successful publication, so a failed run can
retry it. No new persistent state or revision is introduced.

The tests compare zero and two late discards, require continued closure in idle
frames and verify that a fresh long press opens a new menu. A second test
interrupts the output hook, removes transient popup state and verifies retry
from the unpublished request. The panic fixture explicitly clears the abandoned
output's texture deltas before unwinding. Both tests pass as part of workspace
run `3dc9ae22-02d7-44d8-a455-3df01a8c61c6`.

## Review and verification

Implementation commits:

| Commit | Scope |
| --- | --- |
| `16dcbdd` | Stack-safe native captions and borrowed-view diagnostics |
| `8af8caa` | Single-consumption touch menu intent across owned-run passes |
| `b6fc6e5` | Prepaid bounded endpoint handoff, exact admission facts and public regressions |

Independent review finds no blocking P1/P2 in the final traversal, capacity or
menu changes. The capacity review explicitly checks direct-predecessor identity,
exact settlement, both admission phases, N+1 accounting and candidate rollback.
Native command review also traces command-slot ownership, obsolete menu epochs,
staged style publication and close protection without confirming another defect.

All commands use `CARGO_BUILD_JOBS=1` and `CARGO_INCREMENTAL=0`. The
[qualification artifacts](2026-11-10-depth-and-handoff-review/) retain failing
reproductions, targeted results, full test logs and dated command receipts.
The failing touch transcript also records the initial panic-fixture texture
cleanup issue, which was corrected before final verification.
Clippy gates deny correctness and suspicious diagnostics; the verbose workspace
style output is retained as an explicitly labelled final-output excerpt. Log
normalization removes trailing whitespace and terminal blank lines only.

| Gate | Result |
| --- | --- |
| Workspace, all features and targets | 2,306 passed; three existing skips |
| Independent persistence host, all targets | 343 passed |
| Official/product consumers | 24 / 114 default / 119 persistence passed |
| Pinned native fork, all features and targets | 593 passed; one existing skip |
| Workspace correctness/suspicious Clippy, strict docs, eleven doctests, format and dependency boundary | Passed |
| Native correctness/suspicious Clippy, strict docs and native/host format | Passed |
| Actual Linux X11, bounded redock and complete editor | Seven real eframe processes, 4 MiB profile and both editor drivers passed |

The [workspace receipts](2026-11-10-depth-and-handoff-review/workspace-gates.json)
and [consumer receipts](2026-11-10-depth-and-handoff-review/consumer-gates.json)
record all fifteen final serial gates. The initial obsolete-test failure is
retained separately. The
[X11 receipts](2026-11-10-depth-and-handoff-review/real-x11-gates.json) record
thirteen successful display, build and runtime steps. Actual GUI work uses the
existing isolated Linux container, limited to one CPU and 4 GiB, with a
1920x1080 Xvfb/Openbox display, software Glow and one llvmpipe thread. Development
and test builds disable debug information. The exact fresh test executable is
selected from Cargo's compiler-artifact JSON; every real test runs in its own
process with `DISPLAY=:99` and the X11 backend.

The [bounded redock log](2026-11-10-depth-and-handoff-review/native-redock-bounded.log)
confirms creation, first-live, redock and retirement under the selected 4 MiB
storage profile. The
[complete editor log](2026-11-10-depth-and-handoff-review/actual-editor-lifecycle.log)
confirms twelve real focused edits, six destroyed child lifetimes, close/reopen,
lock/reset, JSON placeholder restore/reload, child replacement and Root
rebinding. The original Root survives, every child is destroyed and final
quiescence holds. Implementation source remains unchanged after qualification.

These correctness repairs do not establish a new ImGui performance ratio or
extend physical-platform qualification beyond the profiles actually rerun.
