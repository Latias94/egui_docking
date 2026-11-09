# P2 interaction and native recovery sweep

This follow-up starts from `42cf9b0`. It checks application modal isolation,
keyboard cancellation, native output backpressure, and recovery after a refused
child-window creation. Every retained finding has a regression that exercised
the real failing path before its correction.

## Application focus and interaction

The overflow menu requested its focused row every pass and consumed navigation
and activation keys behind an application `egui::Modal`. Ordinary tabs also read
AccessKit actions directly, and pane-focus requests bypassed modal permissions.
The shared action predicate now includes egui's layer interaction permission;
tab, pane, workspace, and contained controls use it where they accept actions or
request focus. The application keeps ownership of its content and modal state.

Pointer-driven tab focus is applied after docking publication. That final focus
request also needs current widget and layer eligibility. Owned runs inspect the
terminal completed pass (`prev_pass`); caller-owned rendering inspects the pass
still being constructed (`this_pass`). `Context::read_response` is unsuitable
here because its fallback can prefer a previous registration after `end_pass`.
The regressions cover both a newly opened modal and a tab disabled by a terminal
retry pass, with later text delivered to the application editor.

Native scroll geometry must survive modal occlusion. Removing its registration
made the fork report `NoReceiver`, contradicting core's underlying menu scroll
region and rejecting the native input prefix. Scroll footprints remain
registered while egui decides occlusion; the real hit snapshot reports `Blocked`.
This geometry does not enable keyboard, AccessKit, click, or focus actions.
The native regression presents a 40-item menu, verifies its unobstructed scroll
receiver, opens a real modal, and passes native wheel input through the actual
receiver store and reducer without changing menu position or pane selection.

The presentation menu had a separate keyboard cancellation defect: Escape closed
the popup but discarded its anchor focus, preventing a second keyboard opening.
An actual popup Escape close now consumes the key and restores the operable
anchor, provided no command was activated. Its regression opens, enters, closes,
and reopens the menu using real keyboard events.

## Native Escape delivery

Only the source surface exposed an Escape action, although core already accepts
an exact native delivery binding from another window. The paint plan now freezes
that binding in the existing prepared/deferred action. Local surfaces retain
their source-only delivery rule. Deferred validation still checks the exact
gesture status, scene, epoch, coordinates and current binding before cancelling.

The two-window regression starts a real native drag, changes keyboard focus
while retaining capture, paints Escape in the selected viewport, and submits the
renderer action through the deferred reducer. It covers both source and peer
delivery, unchanged layout, cancellation feedback, and capture release. This is
an adapter/core regression; it does not claim OS-level cross-window qualification.

## Native input during output backpressure

A real X11 character entered egui while the output slot was full, but the
retained pass skipped the pane callback. Its TextEdit remained empty after
rendering resumed. Input admission now services earlier native work and probes
the existing core output allowance before eframe extracts input. Backpressured
input uses the existing defer/terminal protocol and stays in eframe's buffers.
The unpublished probe produces no UI, output or committed input watermark.
Shutdown, creation, staging and application close retain their progress paths.

A native resize exposed a second branch: an unmeasured scene returned before
checking occupied output slots. That branch now checks the same surviving-output
count used by formal output reservation. It does not guess an unknown endpoint,
reserve an encoding, or replace the Ready branch's stream-capacity check.

The real X11 regressions hold a renderer terminal, send an XTest character, and
release the original receipts from native service. The resize variant also sends
a real native window resize. Both require admission to defer, no Text event to
enter a retained UI pass, and the restored TextEdit to contain exactly `a`.
They run in separate processes because winit does not recreate an event loop.

## Refused child creation

The example treated accepted child creation as an obligation to wait forever for
its first presentation. When the physical resource limit refused creation, core
retired the staging child and retained source panes, but the example left its
toolbar and inventory commands disabled. It now ends that wait when lifecycle
work is quiescent and the target surface no longer exists. A missing surface
alone cannot end a normal staging operation.

The real-window example regression limits physical resources to the root slot,
submits the ordinary window command, observes accepted creation and subsequent
refusal, then checks source ownership, retained pane data and enabled controls.
CI includes dedicated X11 gates for this recovery and the two Text regressions.

## Scope and verification

The final verification receipt and compressed red/green logs are stored in the
adjacent `2026-11-09-p2-interaction-sweep` directory. Independent reviews cover
egui action/focus permissions and native cancellation, input admission and output
slot counting. No additional reproducible P1/P2 remained in those reviewed paths.

Final checks passed on the source hashes recorded in
[source-identity.json](2026-11-09-p2-interaction-sweep/source-identity.json):

- Main workspace: 2,267 tests passed; three skipped.
- Native workspace with all features: 581 tests passed; one skipped.
- Native workspace with default features: 570 tests passed; one skipped.
- Product integration harness: 119 tests passed.
- Both workspace Clippy correctness/suspicious gates, formatting, documentation
  tests, workflow YAML parsing, and `git diff --check` passed.
- Ten isolated Linux checks passed: final example build, actual-pass publication,
  framebuffer backpressure, real UI-ingress retention, both Text regressions,
  native close/create boundary, refused example creation, public command smoke,
  and the complete editor qualification.

The final editor qualification performed 12 real focused edits across six child
window lifetimes, including close recovery, reopen, arrangement lock, default
reset, named JSON restore, missing-content reload, child replacement and Root
rebinding. It ended with preserved pane data, fresh presentation and quiescence.
The [verification receipt](2026-11-09-p2-interaction-sweep/verification.json)
contains exact commands, return codes and durations.

Representative red/green evidence:

| Path | Red | Green |
| --- | --- | --- |
| Modal and terminal-pass focus | [Disabled terminal tab](2026-11-09-p2-interaction-sweep/terminal-disabled-red.log.gz) | [Six interaction regressions](2026-11-09-p2-interaction-sweep/terminal-focus-final.log.gz) |
| Native modal scroll | [Rejected wheel input](2026-11-09-p2-interaction-sweep/modal-scroll-red.log.gz) | [Blocked wheel accepted](2026-11-09-p2-interaction-sweep/modal-scroll-fix.log.gz) |
| Peer-window Escape | [Source passes, peer fails](2026-11-09-p2-interaction-sweep/native-escape-red.log.gz) | [Both deliveries pass](2026-11-09-p2-interaction-sweep/native-escape-fix.log.gz) |
| Backpressured text after resize | [Lost character](2026-11-09-p2-interaction-sweep/retained-resize-probe.log.gz) | [Both X11 text paths](2026-11-09-p2-interaction-sweep/retained-text-final.log.gz) |
| Failed child creation | [Controls stay pending](2026-11-09-p2-interaction-sweep/example-create-failure-red.log.gz) | [Controls recover](2026-11-09-p2-interaction-sweep/example-create-failure-fix.log.gz) |

The native executable checks use the pinned fork on Linux X11/Glow with a real
window manager and software rendering. The changes do not establish equivalent
OS qualification for other backends or update docking benchmark comparisons.
