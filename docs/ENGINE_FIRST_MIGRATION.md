# Engine-first breaking migration

The renderer-neutral `dockspace` crate owns layout and interaction. An engine
owns its renderer, input facts and optional OS resources. Egui and the native
adapter exercise the same public update and presentation contracts.

## Ordinary edits and visible input

An ordinary logical drag, contained move or resize finishes in the successful
update that accepts release. Its delivered result is emitted once. A subsequent
press cannot cancel it, and a later `Dropped` renderer result cannot undo it.
Creating or retiring a native resource remains a separate transfer with source
reservation, exact first-live presentation and compensation obligations.

Input still names the output that was actually visible when the event happened.
The core rechecks the relevant current object, policy, ownership and geometry.
An old group `[A, B]` cannot capture a newly added C. A single-item action on B
can survive unrelated changes when B retains its exact relevant scope. Never
substitute an unpresented current layout for the event's visible hit target.
Manipulation and hover routes are independent event-time observations.

## One update and one publication

Use a single `DockspaceHostFrame` to accept presentation reports and semantic
input, then measure, encode and publish:

```rust,ignore
let mut frame = session.begin_host_frame()?;
// Submit ordered pointer facts and application/deferred actions here.
frame.begin_encoding_pass()?;
frame.measure_surface(surface, metrics)?;
match frame.surface_work(surface)? {
    SurfaceWork::Ready(plan) => {
        let (recording, receipt) = renderer.record(plan)?;
        frame.confirm_surface_painted(receipt)?;
        staged.push(recording);
    }
    SurfaceWork::Deferred(_) => {}
}
frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
let publication = frame.commit()?;
// Only the matching published output permits authorize renderer submission.
```

`integration/dockspace-host-harness/examples/engine_host.rs` is an executable
consumer of this sequence. Measurement immediately supplies the private
candidate's compiled plan; it needs no measurement-only publication. Later
semantic commands belong to a subsequent update after encoding starts.
Unrelated surfaces may remain deferred.

`DockspaceHostFrame::suspend` transfers the same private candidate into an owned
`DockspaceUpdate`. `DockspaceSession::resume_update` borrows its originating
session again. An outstanding owned update excludes a second update; dropping it
rolls back its input, scene and producer watermarks. A foreign session cannot
resume it.

An actual repeated renderer pass uses `restart_encoding_pass`. This retains the
accepted input while discarding the prior pass's derived encoding state. Old
execution receipts cannot confirm a later pass, even if geometry is identical.
Output identity allocation never rolls back: discarded passes and abandoned
updates cannot mint a replacement geometry under an earlier output ticket.
These opaque identity gaps are separate from committed input/producer watermarks.
Application pane callbacks run once per actual pass; errors do not automatically
replay them. Core rollback cannot undo application side effects inside callbacks.

Each explicit workspace command batch prepares one full mutable document copy
and canonicalizes/validates it in place. Separate accepted semantic operations
can each prepare their own atomic batch; this is not a promise of one graph copy
for an arbitrary host update containing many independent mutations. Stable
read-only updates share the immutable document.

## Output ownership and backpressure

Every admitted `PaintedSurfaceOutput` retains its exact immutable scene until its
own terminal result. Report `Presented` or `Dropped` using that affine output;
never acknowledge by a scene revision guessed from the latest candidate.
Out-of-order results retain emission order. Revocation prevents old output bytes
from lending input authority to a replacement host or epoch.

The default output capacity is 64. `configure_output_capacity` can select a
different nonzero bound before admission; shrinking below retained obligations
fails. Query `surface_work` before invoking expensive content callbacks.
`Deferred(OutputCapacity)` is ordinary backpressure, and
`Deferred(Unmeasured)` requests measurement. A zero-output update may still
publish accepted logical input and protected terminal progress. Wake rendering
when the output owner releases capacity rather than continuously retrying panes.

## Deferred UI actions

Convert a `PreparedSurfaceAction` into a `DeferredSurfaceAction` when an action
is produced after the update enters encoding. Submit it in the next update with
`submit_deferred_surface_action`. It carries a shared immutable scene witness;
it does not install that scene as current or create another registry.

The reducer validates the action's relevant source and target. Unrelated global
revisions need not invalidate it. Changed membership, geometry, policy, owner,
popup identity or epoch do. Expected stale input produces `StaleRejected`
without poisoning independent input. Cross-surface product actions whose
destination is outside the source witness retain strict version validation.
Adapters deduplicate repeats of the same originating UI event, not equal intents
from independent runs.

## Egui integration

Prefer `Dockspace::run_single_surface(context, raw_input, surface, panes)` when
the application owns the complete run. The adapter holds one private candidate
across actual egui repeat passes and validates the terminal output after hooks
before publication. It returns a `DockspaceOutput` plus `DockspaceResponse`.
The renderer reports the complete output's actual terminal result through
`DockspaceOutputReceipt`. The official caller-owned demo uses this entry point.

`show_single_surface` is the limited callback integration. It publishes each
accepted call independently and cannot roll it back after an enclosing run or
late hook fails. The composed `egui_dockspace::run_ui` wrapper can report exact
output settlement but does not give these callbacks owned-run atomicity.
Unmodified eframe callbacks cannot invent terminal presentation evidence and
report `PresentationSettlementRequired` for that capability.

Pane painting must be synchronous and append-only on the supplied Ui layer.
Adapter-owned popup chrome may append to registered foreground layers; the
same proof freezes each suffix and preserves all pre-existing prefixes and
transforms. Unknown or modified layers, removed payloads and output-hook changes
cannot authorize a complete surface. The native driver owns an exact
post-hook finalizer and conserves native ownership separately from logical edits.

## Close, focus and persistence

Deferred close approvals validate the captured target membership, lifetime and
policy. Selection, MRU, scrolling and independent edits no longer expire them.
Current host focus, focus history and pending focus intent are separate. Hosts
can report pane/workspace, external, absent or unknown focus explicitly.
Removing a background target does not infer that it owned current focus.
Egui samples actual pane/workspace ownership before semantic actions; reading
`PaneView::focus_target` is an observation, not a focus-request counter. Exact
request acknowledgements still require their final output and terminal report.
Skipped UI passes retain an already focused target only through metadata owned
by that exact presented output, without registering hit receivers or requesting
focus. A same-window Root continuation carries a current physical focus fact to
its successor binding atomically in the core and native producer. Revoked or
foreign facts and old effect acknowledgements do not become successor authority.
Replacing a layout retains the platform provider's replay watermark and any
Foreign/None or revoked focus facts; only replacing the provider resets its
observation namespace. Clearing old pane/host scopes retains an explicit unknown
current-focus fact, so repeated layout replacement cannot turn revocation into
the initial no-history fallback.

Successful document publications reconcile hosting and recovery-host references.
Retired recovery hosts do not leave a save operation with dangling references;
live native resources still finish their explicit transfer or recovery protocol.

The complete-update cost methodology and retained-memory definitions are in
[`performance/2026-11-03-engine-first-methodology.md`](performance/2026-11-03-engine-first-methodology.md).
