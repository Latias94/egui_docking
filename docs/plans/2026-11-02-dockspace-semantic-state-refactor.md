# Dockspace semantic state and engine-host refactor

Date: 2026-11-02. Status: active; first functional tranche qualified; structural work remains open.

This follows the [independent ImGui comparison](../audits/2026-11-02-dockspace-imgui-review.md),
which demonstrates nine public-path counterexamples against `ca8f0c3`. It adds
work discovered after the earlier plan's acceptance checkpoint. Breaking interfaces,
deletion of superseded paths, and fork changes are allowed. The primary consumer
remains an engine-owned UI/render loop; egui is the validation adapter.

## Execution status

The full plan is the active Goal. No slice is qualified merely because its code
has landed in the working tree. Cargo and real-window work have one main-thread
owner; implementation and read-only review lanes use disjoint write scopes.

- P0: all nine public-path counterexamples are maintained host-harness tests;
  same-prefix, separately settled and abandoned-settlement cases now pass.
- P1: last-Presented settlement and owner-only popup visuals are implemented,
  including non-owner click/drag dismissal and wheel blocking. The duplicate
  popup proof map and its revision issuer are deleted. The single pending-mutation
  enum and shared presentation tracker are implemented and qualified.
- P2: defaults and document restore preserve semantic IDs and merge allocation
  history; the rebase module is deleted. Host and managed window keys use distinct
  namespaces in the existing registry; importing managed keys is rejected.
  First-tranche core/public/product/native qualification passes.
- P3: contained movement shares manipulation geometry, zero-displacement resize
  preserves weights, and unrelated scroll preserves/rebinds stationary work only
  through a fresh exact output. Product declaration clone/compile/drop and production
  measurement are iterative; the 10,000-level public scene passes. Nested touching-boundary
  resize remains open.
- P4: native preparation uses continuous relevant facts in the existing streams;
  actual source recovery cancels all create phases even after readiness loss.
  First-tranche native model and finite macOS/X11 qualification pass. Work-area selection
  now uses greatest rectangle intersection and nearest fallback; its structural-tranche
  qualification passes.
- P5: explicit empty-central attach/retire, exact old-output rejection and unaffected
  surface proof retention are implemented. Existing aggregate requirement revisions
  mint fresh per-surface revisions without a second history map. An independent review
  found a drag-reservation conflict gap; four real press/move/release counterexamples
  fail before the fix and pass with both active and pending reservation checks.
  Whole-engine candidates now share the immutable workspace; actual command staging
  retains its owned graph copy. Structural qualification passes.

First-tranche qualification passes workspace 1,970, public default 183 / serde 211,
product serde 117, native normal/noassert 540 each, ten doctests, strict rustdoc,
Clippy correctness/suspicious and formatting. Finite macOS and X11 default/bounded
redock and all six pressure modes pass, as does the X11 physical guide journey.
The W16/O256 gate additionally found and fixed ordinary-output pressure rejecting
an already-owned Show callback; its terminal keeps the existing prepaid owner.
Full records and limits are in [VALIDATION](../VALIDATION.md#2026-11-02-semantic-state-refactor-first-tranche).

Structural-tranche qualification passes workspace 1,988, public default 202 / serde
230, product serde 117, native Linux normal/noassert 540 each, ten doctests, strict
docs, correctness/suspicious Clippy and four formatting workspaces. The five ignored
native opt-in/isolated-renderer cases retain their own qualification scope. Optimized
X11 default/bounded redock, six pressure cases and physical guide docking pass.
Nested touching-boundary resizing and final full-plan delivery remain open.

Independent CI changes and `docs/reviews/` are excluded from all slice commits.

## Required invariants

1. Semantic Item/Root/Surface addresses retain their declared product meaning.
   Runtime instance authority is carried by existing engine domains, workspace
   epochs, scene/requirement revisions, output streams, and native incarnations.
2. Settled-through progress and last actual Presented output are independent.
   Grouping terminal reports into one frame does not lose a presentation.
3. An unavailable unrelated surface cannot prevent interaction with a current
   popup owner. Semantic blocking and visual presentation have separate roles.
4. Existing contained-source movement has one validated geometry result.
   Content, preview, hit/semantic geometry, and terminal mutation consume it.
   Moving, resizing, and initially placing content have explicit sizing rules.
5. Zero effective resize displacement preserves persistent size intent. Motion
   that clamps back to zero has the same property.
6. An input may revoke timed work only through its actual owner/affected scope.
7. Repeated identical native facts preserve in-flight preparation. Genuine
   revocation, provider change and resource failure remain fail-closed.
8. Host resource values do not control internal identity allocation.
9. Logical surfaces can be attached and retired without replacing unrelated
   topology or the whole workspace epoch.
10. A failed or abandoned frame publishes no partial graph, authority, producer
    watermark, focus, or lifecycle transition.

## P0 — Adopt the counterexamples

Move the nine minimal public counterexamples into the existing harnesses,
retaining normal controls and exact failure assertions. Share only concrete fixture setup;
do not create a generic GUI journey language. Add frame-partition tests for
presentation results and repeated-fact tests for native preparation.

Replace tests that encode the old workspace-wide popup presentation barrier with
tests of the new owner/scope contract. Keep negative tests for stale outputs,
foreign owners, popup blockers, abandoned candidates and cleanup successors.
Passing an obsolete behavioral test is not a reason to preserve its model.

Exit: every confirmed finding has a maintained failing oracle, with its contract
and expected terminal result stated independently of the current implementation.

## P1 — Give presentation one coherent settlement model

Addresses F2 and F3.

- Retain the last Presented emission while advancing the contiguous retirement
  frontier through later Dropped results. Apply existing scene-current checks
  before granting authority.
- Separate popup semantic capture/dismissal from per-output visual proof.
  Default visual dependency is the popup owner. Only actual affected output
  domains participate in visual replacement or retirement. Non-owner semantic
  blocking does not require painting a fake backdrop on every surface.
- Delete the workspace-roster rendezvous and the requirement to refresh popup
  measurement on unrelated surfaces. Keep exact owner-output proof for menu
  interaction and any old-owner visual which genuinely needs replacement.
- Factor common pending-presentation progress out of drag release, contained
  release and root rehome. These currently repeat emitted-output sets,
  presented-output selection, failure flags and mutual-exclusion checks across
  three Option fields. Prefer one pending-mutation enum and a concrete shared
  presentation tracker, with variant-specific terminal commands. Do not add a
  second settlement owner or a universal workflow framework.

Exit: all Presented/Dropped combinations preserve their meaning across frame
grouping; hidden/deferred B cannot stall A's popup; stale and superseded visuals
never authorize an action; release delivers exactly once.

## P2 — Separate semantic identity from lifetime evidence

Addresses F1 and F7, and establishes the basis for incremental surfaces.

- Audit every consumer of RootId, SurfaceId and FloatingPresentationId. Determine
  where the existing domain/epoch/revision/stream proofs already prevent reuse.
  Keep declared semantic identities stable across defaults and document restore
  where those proofs suffice; delete the corresponding numeric rebasing and
  high-watermark compatibility behavior.
- Policy is associated with semantic identities. Any remaining intentional
  identity transformation must atomically transform source/target/surface rules,
  presentation preferences and native associations as one replacement. Do not
  leave policy remapping as a host repair step.
- Split host resource keys from managed-window identities inside the existing
  viewport registry. External token values do not advance an internal issuer.
  Preserve duplicate-resource detection, reuse after exact retirement, and
  incarnation-safe late-callback rejection.
- Remove old tests which require public semantic names to drift merely to prove
  staleness. Replace them with explicit old-action/output/binding rejection tests.

Exit: source/target/resize/close rules survive close/default/document cycles;
u64::MAX external tokens cannot prevent a child reservation; old capabilities
remain inert after replacement and semantic-ID reuse.

## P3 — Use one manipulation geometry result

Addresses F4, F5, F8 and F9; qualifies C1/C2 before broadening claims.

- Compute existing contained movement once from the validated manipulation route
  and original press offset. Delete the hover-based recomputation for that source.
  Hover remains responsible for selecting a docking destination. New contained
  placement uses its explicit destination coordinate mapping and sizing policy.
- Give existing movement, resize and initial placement distinct requests to one
  geometry module. Complete content, preview and commit use the same result.
  Delete the duplicate movement/placement clamping paths that disagree today.
- Preserve original split weights on zero effective displacement. Track a user
  resize intent separately from a constrained pixel projection.
- Implement and compare a touching-boundary resize solver for nested splits.
  Share its atomic set of updates across pointer handles, junctions and semantic
  adjustment. Keep the N-ary graph and current ResizeSplits transaction.
- Resolve scroll ownership before changing a drag continuation. Pause only the
  same affected strip/menu. Delete unconditional global timer clearing.
- Consolidate production subtree minimum calculation with the existing iterative
  layout measurement. Delete duplicated recursive traversal. If a depth limit is
  chosen, enforce it consistently with a typed error at all accepted inputs.

Exit: distinct manipulation/hover, oversized restored content, all resize
directions, constrained zero delta, nested resize and unrelated scroll cases
have public immediate/deferred tests; rejected and abandoned frames roll back.

## P4 — Preserve native preparation continuity

Addresses F6; validates C3 separately.

- Bind staging validity to a continuous interval of relevant facts in the
  existing observation streams. Identical snapshots retain an in-flight request.
  Unknown, visibility/geometry change, provider replacement, renderer failure,
  and missing retained resources revoke it.
- Reuse existing observation generations and effect-emission fences. Avoid a new
  readiness clock or registry. Extend the direct-preparation continuity idea to
  staged preparation where the facts justify it.
- Keep shared NativeBringupPhase and its reducer. Keep distinct transfer and
  recovery owners unless a concrete duplicated transition can actually be deleted.
- Evaluate default placement using authoritative work-area geometry: intersection
  and nearest-area fallback with deterministic decisions where authority permits.
  Treat this as a separate algorithm change with its own negative cases.

Exit: PreShow/PostShow complete through arbitrary snapshots with unchanged
durable facts; Hidden→Unknown→Hidden, geometry changes, failed preparation and late predecessors
cannot revive old output; DirectPrepared/Staged share exact first-live transfer.

## P5 — Complete the self-drawn host seam and reduce transaction copying

Addresses D1 and D2 after the semantic contracts above are stable.

- Add explicit logical-surface attach and retire operations to the existing
  host-frame transaction. Empty central dockspaces are valid containers. A
  nonempty retire requires an explicit refusal, close decision or migration.
  Native hosting remains a separate association of an existing logical surface.
- Require an S1/S2 public journey: present S1, attach empty S2, dock into S2,
  empty and retire S2. S1's unaffected input proof remains usable and the workspace
  epoch does not change just for surface membership.
- Keep immutable compiled geometry shared. Profile workspace/ledger allocations
  and frame-prelude work; replace full candidate copies with copy-on-write or a
  scoped change journal only where the profile supports the change.
- Preserve one canonical graph and atomic publication. An optimization must not
  create a second mutation authority or weaken rollback to save copies.

Exit: renderer-neutral dynamic surface management is demonstrated without native
enrollment; idle and touched-surface cost is measured at 16/128/1024 panes;
the existing structural, overload and lifetime gates still pass.

## Verification and delivery

Each slice ends with its public regression tests, affected core/adapter tests,
formatting, documentation and an independent read-only review. Only the main
thread runs Cargo or GUI work, with one build job and serial nextest execution.
Native slices refresh both default and assertion-disabled behavior and the
corresponding finite real-window checks; no untested platform is relabeled ready.

The final qualification refresh includes core/public/product/official consumers,
dependency isolation, strict docs, matched extracted packages, native lifecycle
and the enabled X11 physical profiles. Full matrices are refreshed at meaningful
source freezes, rather than after every reversible change.

Keep the earlier plan and its receipts as history. Update current readiness
claims from this plan's actual results. New tests begin red; the plan is complete
only after the corrected semantic contracts, obsolete-path deletion and final
qualification all agree. Open-GPUI and unenabled platform certification retain
their existing separate scope.
