# Dockspace and ImGui design review

Date: 2026-11-02. Status: review complete; remediation not implemented.

## Scope and evidence

Reviewed dockspace `ca8f0c318ac3114a3867077af9c5813a7b29b8da` against the
local docking reference ImGui `d88f238d1d363ef387382b2e29ad12cf96f826ac`.
Three independent read-only reviews covered interaction, native establishment,
and topology/layout/persistence. The main review covered frame transactions,
presentation settlement, and public engine-host integration, then reproduced
the strongest findings through the public runtime interface. A fourth independent
read-only review challenged the fixtures, findings and proposed authority rules;
it found no fixture false positives and required the evidence wording below to
distinguish unchanged durable native facts and intentional popup contract changes.

The earlier completion audit records the previous acceptance matrix. Its passing
results do not cover the interleavings below. This review supersedes a blanket
interpretation of that checkpoint as current correctness approval.

No production Rust code, reference checkout, lockfile, CI change, or existing
review artifact was modified. Reproduction code lives in the temporary consumer
`/tmp/dockspace-imgui-review-czr9kvzb`. It depends on the actual dockspace and
public host harness through path dependencies. Fixtures copied into that
consumer use the same public interfaces as the repository tests; they do not
expose private engine state or mint authority. Cargo work was serialized with
one build job and one nextest test thread, reusing the project target directory.

The final combined run executes **13 tests: four passing controls and nine
failing new expectations**. An existing preview test also matches the `review_`
substring filter and passes. These are executed public-path counterexamples
against unchanged production code. They expose functional defects and a popup
liveness design flaw; they do not establish that an earlier version behaved
correctly. The probes were not added to the main workspace.
The first oversized-contained attempt fails on a fixture assumption that all
resize handles are visible. The corrected probe accepts clipped chrome and
fails on the actual 1000-versus-960 geometry discrepancy.

Local evidence:

- `/tmp/dockspace-imgui-review-state.json`: commands, source identities,
  reproduction source hashes, outcomes, and performance samples.
- `/tmp/dockspace-imgui-review-czr9kvzb/confirmed-review-probes.log`: final
  combined public-path counterexample run.
- `/tmp/dockspace-imgui-review-czr9kvzb/contained-public-probes-v2.log`:
  corrected contained geometry probes.
- `/tmp/dockspace-imgui-review-czr9kvzb/idle-cost-release.log`: scoped release
  samples, not a latency qualification or an ImGui benchmark.

The probe command is:

```text
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=<repository>/target
cargo nextest run --manifest-path <temporary-consumer>/Cargo.toml
  --locked --offline --test-threads=1 --no-fail-fast
  -E 'test(review_) | binary(presentation_prefix) | test(staged_existing_surface_requires_pre_show) | test(host_local_title_move_compiles_the_complete_split)'
```

The environment assignment and command above describe one invocation; the
temporary receipt preserves its exact argument array. Temporary files are local
evidence, not portable repository assets. The remediation plan must move the
minimal counterexamples into the maintained public consumers as regression tests.

## Confirmed functional defects and liveness design flaw

### F1 — P1: restoring defaults silently removes root-scoped restrictions

`engine/workspace_rebase.rs:70` gives a restored, retired RootId a new numeric
identity. `runtime/layout_reset.rs:253` replaces only the workspace.
`engine.rs:3830` installs that graph while retaining policy.
`policy.rs:1651` looks up the new root and treats a missing source rule as
permissive. Surface-scoped rules have the same structural exposure, although
the executed probe exercises a root source rule.

The public probe disables source operations on root R2, confirms a dock attempt
is rejected, closes the root, and restores the same defaults. The restored root
is R3. Moving it now returns `ProductActionApplied(RootDocked)` instead of
`PolicyDenied`.

The root cause is that public semantic addresses also carry retirement policy.
Stable layout identity and the authority of one live instance need separate
meanings. ImGui reconstructs docking settings by persistent node ID
(`imgui.cpp:18606`) and applies caller flags/class at `DockSpace`
(`imgui.cpp:20945`). This is a useful separation to adopt without importing its
global context.

Reproduction: `review_root_policy_survives_closing_and_restoring_defaults`.

### F2 — P1: a later dropped output erases the last actual presentation

`runtime/presentation.rs:487` advances the settled frontier and then unconditionally
replaces `presented`. A Dropped result replaces the preceding Presented key with
None. `compile_report` at line 665 coalesces the complete prefix through this
method.

The public probe emits two identical semantic scenes before reporting their
results. Presented(A), Dropped(B), then one settlement retires both without
granting A receiver authority. The reverse Dropped(A), Presented(B) control
passes. Retirement progress and last actual presentation are independent facts;
one must not overwrite the other.

ImGui has no equivalent asynchronous result protocol, so this is a dockspace
contract defect rather than missing ImGui source code to copy. Preserve exact
output identity and stale rejection while correcting prefix accumulation.

Reproduction: `later_dropped_output_preserves_the_last_actually_presented_output`.

### F3 — P1: an unrelated non-rendering surface prevents popup interaction

`scene/lifecycle.rs:536` stores every workspace surface in `PopupInteractionGate`.
`try_present_popup_interaction_gate` at line 1418 requires a current proof from
every member. Popup opening and closing revoke all interaction until that
complete roster crosses the barrier. `scene_manifest.rs:823` also requests popup
plane measurement on every surface.

The public probe presents A and B, opens a tab-list menu on A, and presents A's
exact popup output. It intentionally leaves B without a successor output.
A's `presented_surface` remains unavailable. A minimized native window, paused
render target, or independently scheduled surface can therefore stall a visible
menu indefinitely.

The workspace-wide barrier is intentional in the current implementation and
tests. This counterexample identifies a liveness flaw for independently scheduled
surfaces; remediation deliberately changes that contract.

ImGui separates popup input blocking (`imgui.cpp:5071`, `5767`) from executing
the popup window (`13448`); it does not require every viewport to present a
successor. Dockspace should retain global dismissal/blocking semantics when
appropriate, while making visual authority depend on the actual popup owner
and affected output domains.

Reproduction: `review_popup_on_a_is_interactive_when_b_has_no_new_output`.

### F4 — P1: contained content and its commit preview use different routes

`engine/contained_projection.rs:61` derives the moving source rectangle from
validated manipulation coordinates. `engine/pointer_transaction.rs:2691`
passes the hover point into the non-docking contained candidate.
`engine.rs:4600` independently translates the source rectangle from that point.

The public probe supplies two valid same-surface routes with a 120-point
horizontal difference, disabling magnetic acquisition to isolate source motion.
Complete content projects to `(256,172,400,300)` while the contained commit
preview is `(376,172,400,300)`. The ordinary equal-route title-move control
passes. The differing preview is already observable before terminal delivery;
the probe does not claim to have executed its eventual jump on release.

ImGui moves the source from mouse position minus the original press offset
(`imgui.cpp:5642`); target hover does not reconstruct source geometry. Existing
contained motion should have one manipulation-derived result consumed by
content, feedback, and terminal command preparation.

Reproduction:
`review_contained_move_uses_manipulation_for_both_projection_and_commit_preview`.

### F5 — P2: moving an oversized contained source changes preview size

`engine/contained_geometry.rs:231` preserves dimensions during movement.
`clamp_contained_rect` at line 32 limits dimensions to the host bounds, and
`engine.rs:4605` uses this placement path for the same existing source's commit
candidate. Valid imported or previously larger layouts can exercise both paths.

The corrected public probe moves a 1000-point-wide contained split on a
960-point surface. Complete content remains 1000 wide while its commit preview
is 960. Moving an existing window and placing a newly created floating group
need explicit, distinct sizing rules. If automatic shrinking is desired, both
projection and command must consume the same result.

Reproduction: `review_contained_move_of_oversized_source_preserves_one_width`.

### F6 — P2: identical native observations revoke in-flight staging

`runtime/native.rs:1341` advances window observation generation for each live
snapshot. `frame/native_bringup.rs:449` and `579` replace PreShow/PostShow requests
when that generation changes, even if the facts remain identical. The current
proof check at line 781 rejects the older in-flight output.

The public probe inserts another Hidden/Created snapshot between PreShow emission
and its Presented result. The durable window state, readiness and coordinate
facts are unchanged; the initial one-shot Create acknowledgement has already
been consumed and is absent from the second snapshot. No Show effect follows.
The original staged lifecycle control succeeds. A host that always observes
another snapshot before an asynchronous result arrives can repeatedly lose
progress.

ImGui's `UpdatePlatformWindows` (`imgui.cpp:17830`) retains creation/renderer state
across repeated observations. Dockspace should prove a continuous validity
interval using the existing observation streams, preserving emission fences
and revoking on actual state, geometry, provider, or renderer-readiness changes.

Reproduction: `review_identical_hidden_snapshot_preserves_inflight_pre_show`.

### F7 — P2: external resource values can exhaust internal window allocation

`HostWindowToken::new` (`runtime/native.rs:75`) accepts any u64. Registration at
`viewport_registry.rs:556` merges that value into the internal `last_token`.
Child reservation at line 583 then uses its checked successor.

The public probe registers the external root with u64::MAX successfully and
subsequently fails the tear-off input reduction. This is premature internal
exhaustion caused by one host value, not exhaustion after many live instances.
ImGui separates viewport `ID` from `PlatformHandle`
(`imgui.h:4261`, `4281`). Separate these identity domains inside the existing
registry; retain its sole incarnation authority and exact duplicate-resource
checks.

Reproduction: `review_external_root_token_cannot_exhaust_core_child_identity_space`.

### F8 — P2: a zero-displacement splitter click rewrites persistent weights

`engine/splitter_geometry.rs:132` reconstructs weights from constrained projected
extents even for zero effective delta. Release at `pointer_splitter.rs:263`
always builds and applies that proposal.

The public probe uses a 400-point available span, weights `[0.25,0.75]`, and
minimum widths `[200,100]`. Clicking and releasing the splitter at the identical
point rewrites the weights to `[0.5,0.5]`. Later growth therefore uses a different
layout despite no resize intent. The existing no-Move test releases 48 points
away and does not cover this condition.

Preserve original weights when effective displacement is zero. Treat persistent
size intent and constrained projection separately. ImGui's `SizeRef` writeback
at `imgui.cpp:20798` is not evidence that this desired property holds there;
this finding is based on dockspace's durable-layout semantics.

Reproduction: `review_zero_displacement_splitter_click_preserves_durable_intent`.

### F9 — P2: unrelated blocked scrolling deletes another pointer's wake

`engine/scroll_interaction.rs:339` clears the active drag continuation before
checking the scroll receiver, consumption, stream owner, or affected strip.
`pointer_timed.rs:380` removes the sole timed request. The generic scroll branch
in `pointer_transaction.rs:137` is reachable for another pointer.

The public probe keeps pointer 1's active edge-scroll request, then submits
pointer 2's framework-blocked scroll on an unrelated source area. Workspace and
the drag remain unchanged, but the previous request becomes None. A stationary
drag cannot restart it without fresh motion. Cancel only work whose actual
strip/menu ownership is affected; do not add another timer registry.

Reproduction:
`review_unrelated_blocked_scroll_preserves_the_drag_owner_continuation`.

## Further issues and design gaps

These have code evidence but were not demonstrated by a failing execution in
this review. They must not be mixed into the count of nine executed public-path
counterexamples.

| Item | Evidence and implication | Required next verification |
| --- | --- | --- |
| C1: nested splitter locality | The current resize proposal changes only incident SplitterRecords, after which descendants redistribute by their weights. In `H(A,V(B,H(C,D)))`, changing the outer boundary can resize distant D. ImGui explicitly collects touching leaves and preserves other siblings with `WantLockSizeOnce` at `imgui.cpp:20770–20824`. | A public nested-layout geometry comparison. Classify this as an ImGui interaction gap until the desired resize contract is fixed. |
| C2: recursive production measurement | The low-level solver accepts 10,000 nested splits iteratively (`tests/layout_solver.rs:810`), but scene compilation recursively calls `subtree_minimum` (`scene_compiler/surface.rs:2023`). The public layout declaration compiler also recurses. | Isolated subprocess through public restore/measurement; choose iterative implementation or a consistent typed depth limit. No crash depth was measured here. |
| C3: default native placement selection | `engine/native_tear_off.rs:97` requires exactly one work area containing the requested rectangle's center. ImGui uses rectangle intersection with a fallback (`imgui.cpp:17974`). Source windows spanning a gap can be rejected despite usable areas. | Pure host regression with authoritative disjoint/gapped work areas. Overlapping rosters need explicit authority semantics before selecting a tie-break rule. No broader platform certification is implied. |
| D1: incremental logical surface lifetime | Public actions can target an existing Surface and create its native hosting, but cannot independently attach or retire an empty logical surface. A central empty root can be declared initially, yet content-close rejects an empty root. Ordinary dynamic self-drawn surfaces currently require full layout replacement. | Add public attach/dock/empty/retire journey while retaining unrelated S1 authority and the workspace epoch. This is an engine integration gap, not a bug in the fixed-window example. |
| D2: candidate copying | Every host-frame candidate clones the workspace, policy, ledgers, viewport state and pending operations (`engine.rs:5886`). The compiled immutable plans already use Arc, so this is not a claim that their buffers are all deep-copied. | Profile allocations and dirty versus idle work before choosing a transaction representation. Preserve full rollback and exact producer publication. |

The release-mode idle sample performs no input, no new measurement, no paint,
and no topology mutation after initial setup. Five batches of 200 frames give
median costs of approximately 24.3 microseconds at 16 panes, 75.7 at 128, and
506.2 at 1024. These are local arm64 samples under concurrent machine activity,
not an ImGui comparison, a guaranteed latency, or a pass/fail budget. Together
with the explicit clone path, they justify investigating copy-on-write or
change-journal transactions. They do not justify adding a second layout graph.

## Architecture to preserve

- One canonical N-ary workspace with same-axis normalization. ImGui's binary
  representation is not required for touching-boundary resize behavior.
- One core gesture owner, exact owner/session source continuation, and separate
  manipulation and hover facts. F4 is a consumer-side mixing defect.
- The complete ordered surface program shared by immediate and deferred hosts.
- Exact output disposition, domain/epoch/scene validation, and first-live source
  ownership transfer. ImGui's synchronous frame counters are not substitutes.
- Opaque native binding incarnations, separate Create/Show acknowledgements,
  provider revocation, Missing versus Destroyed, and exact cleanup quiescence.
- The existing shared `NativeBringupPhase` and reducer. Tear-off and recovery
  owners have distinct responsibilities; this review does not establish a need
  to merge their maps or add a universal saga abstraction.

The proposed execution order, deletion targets, and exit conditions are in the
[follow-up refactor plan](../plans/2026-11-02-dockspace-semantic-state-refactor.md).
