# Native payload commands and atomic multi-Root restoration

Date: 2026-11-10 (Asia/Shanghai). Baseline: `6a63fb6`.
Implementation: `1d1abe8`.
Status: G02/G03/G04 implementation, independent review and qualification complete;
G01 remains proposed and unimplemented.

## Implemented scope

G02, G03 and G04 from the
[responsibility review](2026-11-10-docking-multiview-responsibility-review.md)
now have concrete core and public-host implementations. G01 remains the
explicitly proposed [shared native window design](../plans/2026-11-10-shared-native-window-design.md).
This work does not permit multiple logical surfaces to share a managed native
window, and does not extend the physical-platform qualification profile.

### Payload commands and placement

`NativeTearOffSource::{Item, Tabs, Root}` selects one item, an opaque captured
tabs group or a complete root. `PreparedTabsGroup` retains its issuing session,
workspace version and exact source authority. It neither exposes NodeId nor
retargets an anchor at submission. Stale and foreign groups cannot be refreshed
by preparing them again.

Session availability, prepared actions, current frame commands and paint commands
share payload preparation and the existing native creation owner. A partial
payload reserves a fresh root; a complete root preserves its identity. Both keep
source content in place until exact Prepared, visible semantic first-live
presentation. Failure, cancellation, discarded candidates and late outputs use
the original reservation, compensation and resource retirement protocols.

Explicit placement remains a validated physical rectangle. Default commands use
current authoritative geometry, OS insets, work-area selection, cascade and clamp
without entering paint encoding or retaining historical positions. Partial Item
and Tabs payloads use their pane geometry and corresponding measured minimum;
they do not inherit the enclosing floating window or a sibling's minimum. The
existing pane and contained minimum formulas are shared rather than copied.

The native application exposes the same source choices and default placement
through `NativeCommand`, with group capture under its runtime lock. These
commands use the existing bounded application slot and presentation tracker.
There is no second command queue or docking state machine.

### Batch Root continuation and restoration

`submit_native_document_restore_with_roots` consumes explicit
`(existing_binding, incoming_surface)` pairs. All Root mappings, document
sidecars and Child creation intents publish atomically. Each Child retains its
own declared recovery host. `submit_native_layout_reset_with_roots` provides
the same continuation contract without persistence.

Core rejects duplicate sources/targets, foreign/stale predecessors and targets
occupied by nonparticipants. It validates the complete mapping, vacates all
predecessors, then installs successors; complete Root cycles cannot overwrite
unprocessed records. Exact epoch/incarnation changes, fresh native observations
and presentation authority remain mandatory. Old callbacks keep their original
quiescence obligations.

The native adapter validates and installs continuation batches under its existing
publication lock. It prepays all successor resources, releases speculative
owners after refusal, and preserves each physical receipt, pending repaint and
old callback binding. The reference application's automatic document restore
entry remains its single-Root product profile.

## Findings repaired during implementation

1. A partial central tabs group could preflight as Ready but execution attempted
   to detach its protected central node. Moving Tabs now transfers contents,
   selection and MRU while leaving the source central container empty. Subtree
   detachment remains rejected. The shared operation serves pointer and command
   paths; source reference invalidation and item conservation remain enforced.
2. Session availability did not see the native producer's exhausted binding
   slots. It now reads the existing producer budget; candidate-local admission
   continues to enforce the actual allocation.
3. A partial payload from a contained split inherited the entire floating
   window's geometry/minimum. It now uses exact pane/payload facts and the shared
   minimum formulas. Complete-root behavior is preserved.
4. A foreign group's correct core authority rejection could stop the receiving
   native application. `is_foreign_prepared_action` identifies only that exact
   pre-append error. The application slot returns `Dock(Rejected(Conflict))` and
   remains usable. Phase, protocol, publication and internal errors still
   propagate; no broad `OperationConflict` catch was added.

Legacy error priority is preserved for both placement requests: default placement
requires a native placement provider before solving geometry; explicit requests
retain policy-first rejection. Query and execution share this decision for each
request. Both existing availability assertions remain unchanged.
The first native full-suite run also caught an incorrect new adapter-test
expectation for the no-backend default command. That expectation now checks
`NativeUnavailable`, while the adjacent explicit command still checks
`PolicyDenied`. This was a test-only correction; it changed no production source
after the successful core, public-host and egui/product gates.

## Independent review

Three Codex lanes independently examined core lifetime/recovery, payload commands
and native input/rendering ownership. Implementation had disjoint file ownership;
only the primary agent ran Cargo and prepared commits. Review covered batch
cycles, callback isolation, Child anchors, source/group identity, central contents,
partial geometry, policy, first-live and command error propagation. The foreign
group P2 was repaired and independently rechecked; no blocking finding remained
in the frozen source. Review and reference exclusions were not opened.

## Verification

Commands, timestamps, exit codes and logs are retained in
[the evidence directory](2026-11-10-native-command-root-refactor/).
`source-manifest.json` records the baseline and SHA-256 values of modified source
and consumer files. Cargo runs serially with `CARGO_BUILD_JOBS=1` and
`CARGO_INCREMENTAL=0`, reusing the existing target directory.

The focused public run passes all 44 cases: eleven payload/geometry tests,
seven multi-Root tests and the existing direct-create/document-restore controls.
Six native batch route tests pass, including the two existing single-Root
controls. All sixteen serial gates pass:

| Gate | Fresh result |
| --- | --- |
| Core and host conformance, all features and targets | 2,135 passed; three existing skips |
| Independent public host, persistence and all targets | 387 passed |
| egui adapter, all features and targets | 181 passed |
| Official egui consumer | 24 passed |
| egui product, default / persistence | 114 / 119 passed |
| Native workspace, all features and targets | 603 passed; one existing skip |
| Workspace/native correctness and suspicious Clippy | Passed |
| Strict workspace/native documentation | Passed |
| Workspace documentation examples | Eleven passed |
| Workspace/host/native formatting | Passed |
| Renderer-neutral core dependency boundary | Passed |

`serial-gates.json` is the final successful receipt set. Logs and receipt files
marked `before`, `intermediate` or `focused-after` preserve earlier diagnosis;
they do not replace the final full-gate results. The final adjustment after
the test gates joined one native mailbox import line with rustfmt. No semantic
production code changed, and the final source manifest matches both working
and staged source/consumer files.
Log trailing whitespace and redundant final blank lines are normalized for
repository formatting; commands, diagnostic content and results are preserved.

All thirteen real-X11 gates also pass. Fresh build logs, isolated test-process
logs and `real-x11-gates.json` retain the actual commands and timestamps:

- Seven independent eframe processes pass the existing preparation, final-pass,
  geometry, output-capacity, ingress and focused-editor checks.
- The bounded native smoke completes create, exact first live, redock and
  retirement. Peak resident storage is 2,220,400 bytes and reserved storage is
  3,340,616 bytes, below its selected 4,194,304-byte limit.
- Both complete actual-editor drivers pass. Twelve real focused edits, six
  Child lifetimes, close/Allow/reopen, lock rejection, reset, document restore,
  live Child replacement and cross-surface Root continuation preserve pane
  state. The original Root X11 window remains alive; all Children are destroyed
  and the final runtime is quiescent.

`x11-environment.json` and the monitor log confirm one 1920x1080 X screen with
one active identity-transform CRTC. The reused Xvfb/Openbox/Glow container is
limited to one CPU and 4 GiB; native scale is one. These runs retain the existing
one-surface-per-window physical profile. The new partial-payload and multi-Root
batch semantics have public-host and native protocol evidence, not a dedicated
physical multi-Root swap qualification. Shared-window composition, mixed-DPI
interaction and additional platform profiles remain outside this evidence.

The final source/consumer hashes also match implementation commit `1d1abe8`.
No performance comparison was rerun or extended by this feature qualification.

## Migration and remaining scope

`PresentationCommandPaintRecord` and `NativeCommand` are Clone rather than Copy
because captured groups retain owned opaque authority. Consume records or clone
a command only when deliberately retaining it. Their ordinary scalar inspection
methods borrow. Existing explicit Root convenience entry points remain wrappers
around the shared implementation. No document schema change is required.

G01 still requires a complete window/placement ownership replacement: one real
window identity, multiple exact surface placements, one Joined input order,
surface-qualified output and first-live admission, window-wide close/recovery
and final-placement resource retirement. Its public composition entry must not
be opened before those semantics work together. Multi-Root restore and payload
commands are independently usable capabilities and foundations for that work.
