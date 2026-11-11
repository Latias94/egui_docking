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

Hosts which own fallible native resource preparation can replace `frame.commit()`
with `frame.prepare_commit()`. The returned `PreparedDockspaceHostFrameCommit`
contains the final reduction, including ordinary docking vacancies. Inspect its
exact Root continuations, placement detachments and complete proposed window
rosters, reserve the host's callback and retirement storage, then call its
`commit()`. Dropping either frame rolls back the same input prefix and workspace.
No proposed binding authorizes callbacks before publication.

`native_placement_detachments()` lists previously published exact placements
which now own terminal guards while another binding retains the shared window's
physical obligations, or the window continues through a Root pair. Drain their
exact outputs and callbacks, then report binding quiescence. A surviving sibling's
physical window needs no destruction. Root continuation predecessors are listed
in their pairs instead; the matching committed report confirms both collections.
Placement detachment never releases a physical destruction obligation.
The producer byte reservation includes the detachment report backing; an
allowance set to the previous exact minimum may need to increase.

The original physical callback tag can outlive its placement generation. Validate
window-close dispatch with `recognizes_native_window_binding`, together with the
exact close token and physical receipt. After physical destruction is committed,
`retained_native_surface_bindings_for_window` enumerates the producer's remaining
exact lanes. Check their host references and report each one's quiescence before
releasing the physical retirement owner. A full input buffer delays this drain;
it does not release that owner or recreate a second roster.

Publish changed `set_native_surface_placements` in a separate frame from layout
reset or document restore. A replacement after a placement change returns
`OperationConflict` before consuming the replacement; the placement candidate
may still commit or be dropped. This keeps continuation and retirement
predecessors tied to published exact bindings. An equivalent declaration keeps
its exact identities, saves explicit geometry and may precede replacement.

Submit `replace_policy` and `replace_presentation_config` after the accepted
semantic input prefix and before measurement or encoding. They update the same
private candidate immediately and close semantic input. Earlier pointer input
keeps its original policy and visible facts; the resulting measurements and
program use the new configuration in this update. A dropped update publishes
neither input nor configuration. A restart retains both without replaying input.

`SurfacePaintPlan::measurement_refresh` supplies the per-surface core decision.
Keep measurements for that surface's gesture or exact preview; an unrelated
surface can refresh title and minimum metadata during the gesture. An adapter
may additionally defer measurement while it retains framework output resources.

Pane minimum-size metadata is required for every open tab, including inactive
or unloaded panes. Supply it independently of constructing the content UI.
Missing metadata defers the surface contribution because potential drag layouts
must be exact; it does not block an accepted semantic selection.

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

Each explicit workspace command batch prepares an independent candidate.
The node directory is cloned, while unchanged node payloads, root structures and
placement metadata share immutable ownership. Checked single-item moves repair
their affected paths; general and staged batches retain their conservation and
normalization checks. A late failure discards the entire candidate. Separate
accepted semantic operations can each prepare their own atomic batch; there is
no promise of one candidate for an arbitrary host update containing many
independent mutations. Stable read-only updates share the immutable document.

## Internal complete layout results

This representation change concerns engine internals and in-crate test
consumers. The production `layout` module is private; external hosts obtain
their render work through the public host-frame and surface-plan contracts.

`LayoutProjection::nodes` now owns each node's `NodeResult { rect, constraints }`;
replace separate rectangle and constraint map reads with this single result.
`LayoutProjection::splits` owns the complete split outcomes. These are immutable
root-local result containers rather than mutable `BTreeMap` values. Use their
`get` and `iter` methods; identity-ordered iteration remains deterministic.
`nodes.iter_preorder()` supports allocation-free traversal of the owned result
in depth-first order. When joining it to a live tree, compare the full yielded
node identity with the expected node.

Every projection still materializes fresh node and split values from the current
leaf answers. Its output remains readable after the originating workspace is
changed or dropped. Shared topology supplies storage and lookup information;
it does not authorize commands against a later workspace.

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

An output limit of N permits N ordinary retained streams plus one global stream
for a drained same-surface endpoint handoff. The handoff slot cannot admit an
unrelated surface or another outstanding output. Core still retires the exact
predecessor and reclaims it through quiescence. `NativeReportStorageLimit`
prepays this extra stream and its temporary reporting storage; previously exact
small byte allowances may need to increase. Its output limit remains N.

## Native readiness

Remove `NativeCreateStrategy` from enrollment calls and effect matching. The
core now has one preparation contract: the host reports creation correlation and
current `NativeCreateReadiness` on the exact binding. Report `Prepared` only when
window geometry, visibility and renderer resources are ready together. An engine
can prepare them directly. The native egui adapter owns its hidden, pre-show,
show acknowledgement, visible and post-show procedure inside its existing
window-resource owner.

The old core staging paint requests, painted placeholder outputs and ShowWindow
effect are removed. A placeholder is host renderer work; it cannot publish
docking topology or lend semantic input authority. Creation acknowledgement may
settle before preparation. Current Prepared facts must remain continuous; loss
of preparation, coordinates, visibility or provider authority revokes the ready
basis. Re-preparation needs fresh exact first-live output rather than another
non-idempotent create operation.

Source reservation, binding generations, prospective semantic content, accepted
first-live presentation, atomic source transfer, compensation and final
retirement remain core responsibilities. A dropped output is neither a create
cancellation nor proof of window destruction. Keep their original terminal
owners until each obligation settles.

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
`DockspaceOutputReceipt`. Use `run_single_surface_with_ui` when an application
needs toolbars, sidebars or status panels around the docking area. Its callback
runs once per actual pass before measurement; docking uses the remaining Ui.
The official caller-owned editor uses this composed entry point.

`PaneView::ui` now returns `PaneResponse`. Replace the removed
`docking_content_widgets` callback and persistent per-item ID list with
`PaneResponse::from(&response)` for a single ordinary control, or combine
`with_content` / `with_content_ids` for composite content. An empty response
preserves the old undeclared-content behavior. The adapter still checks each
registration against the current pass, pane layer, enablement and bounds;
the application remains responsible for declaring only ordinary owned content.
`focus_target` and `focus_state` remain separate because native focus sampling
also happens before pane rendering.

Native construction now rejects a renderer style whose geometry or interaction
timing disagrees with the existing session. Create a matched configuration with
`DockStyle::presentation_config()` when initializing the session. This conversion
uses defaults for core-only settings; it must not overwrite a custom existing
session to make a mismatch disappear. The comparison checks only the style's
represented fields, so custom core drag thresholds, magnetic targeting and
scroll line extents remain independent, as do egui colors and ghost offset.
Style values are compared after exact `f32` to `f64` promotion; an unrepresentable
core geometry value is rejected rather than rounded silently.

Native application commands now use `NativeCommand` and
`NativeDockspaceApp::request_command(command)`. Replace ordinary per-operation
request methods with their command variants, for example:

```rust,ignore
app.request_command(NativeCommand::FloatItem { item, surface })?;
// On a later application update, consume the exact result before submitting more.
if let Some(result) = app.take_action_status() {
    handle_command_result(result);
}
```

`SelectItem`, `OpenItem`, `DockItem`, `DockRoot`, `DockRootBack`, `FloatItem`,
`RaiseContained`, `BringContainedIntoView`, `MoveRootToNewWindow`, `CloseItem`,
`CloseRoot`, `DecideClose`, `ContinueClose` and `SetArrangementLocked` share the
same bounded slot. Queue acceptance does not imply command success. The slot
remains Busy until its terminal result is consumed, including while native
migration waits for exact presentation. Consume the result before waiting for
quiescence. A stored command is an intent prepared at admission; it is not a
revision-bound prepared token. Layout reset and JSON restoration retain their
dedicated preparation methods.

`FloatItem` creates contained content on an existing surface. OS-window creation
uses `MoveRootToNewWindow`. Raising does not select a pane, and bringing a window
into view does not raise it. Geometry-dependent operations need current surface
measurement. The fixed application host must survive closing and cross-surface
movement; a refused departure returns `HostSurfaceRequired` without occupying
the slot.

`NativeDockspaceApp::with_view` now returns its callback result directly. Remove
the outer `Option` handling (`flatten`, `unwrap`, or `== Some(true)`) that only
represented the native view's presence. The callback always receives the last
committed view, including before native attachment and after fatal shutdown.
Optional item/root lookups inside the callback remain optional:

```rust,ignore
let host = app.root_surface();
let open_on_host = app.with_view(|view| {
    view.item(item).is_some_and(|pane| pane.surface() == host)
});
```

View and pane callbacks hold the native runtime lock. Read other runtime values
before entering a callback, and submit commands after it returns.

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
`DockspaceSession::pane_focus_request` and the host-frame query read a committed
pane-focus request without reserving renderer output. Prepare an observation
through the request and submit it to the frame; exact intent, domain, epoch,
target and native activation checks still apply. A new private intent cannot be
read through this query until its publication commits.

`PaneView::focus_target` is an observation, not a focus-request counter. The egui
adapter still uses its final output and terminal report to prove the observed
widget focus.
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
