# Independent review of retained surface facts execution plan

Date: 2026-11-06. Reviewed plan: `docs/plans/2026-11-06-001-dockspace-retained-facts-refactor.md`, read during implementation after documentation baseline `34ab23a` / runtime baseline `5484071`. Source references below describe the code inspected during this review; concurrent implementation may move them. No Cargo, tests, benchmarks, Git mutation, network access, nested agents, or repository edits were performed.

## Verdict

Proceed with the chosen whole-facts retention design. It has sufficient Depth for the ordinary unchanged-geometry selection case without replacing the arena or public frame Interface. The real Seam remains the existing egui and complete headless measurement/paint/settlement paths. The declared exclusions are defensible if actual physical click eligibility is verified rather than inferred from semantic selection.

There is no demonstrated architectural blocker to the selected scope. There are several concrete obligations that should be made explicit before treating the plan as implementation acceptance. Most importantly, immutable facts must lose *all* selected-state captures, old output source basis must remain distinct from current command state, and the press/Armed/release path must actually enter the retained branch. None of these calls for a new framework.

The 10% complete click target is a working acceptance target chosen by the root agent, not an original numeric user requirement, a forecast, or a mathematical consequence of this design. This scope plausibly removes enough work to pursue that target; present evidence does not establish that it will meet it.

## Required clarifications and implementation checks

### 1. P1: Removing target-held proofs also removes the existing source of presented-root admission checks

Plan references: representation/source binding at lines 37-45 and structural inventory at lines 49-53.

Current `engine/presented_input.rs:74` obtains the frozen root fingerprint by searching the plan's `DropTargetRecord.destination`. That source disappears when proof-bearing destinations are deleted. It protects splitters, junctions, contained title/resize and drop-guide/target admission through `presented_root_structure_is_current`; the effect is broader than `drop_resolver::rebind_topology_target`.

The resolver's current `rebind_topology_target` also does two jobs: resolve the same typed location against current state, and compare current structure with the frozen target structure while ignoring selected/MRU differences. Replacing it with an unconditional live `capture_*_target` would silently discard the second job. Equal rectangles do not prove equal tab order, split weights, central node or root ownership.

Required implementation:

- Every plan binding owns the immutable workspace/index basis for that output, independently of the shared geometry owner.
- Presented-root checks read that binding's source basis directly; they do not recover proof from a geometry target and do not query the latest engine state for what an old output used to mean.
- Resolve a typed location using both the bound source basis and live workspace/index. Compare the structural facts required by that operation before authorizing it, or conservatively reject a stale basis. Evaluate live drop policy separately, including newly selected item rules.
- Updating the source binding never makes an old emission newly presented. Coordinate, host, epoch, native lifecycle and observation admission remain separate checks.

Minimum counterexample: preserve the same pane rectangles while reordering tab membership or changing a central node, retain the old output, and attempt a splitter/drop action. It must reject the stale structural basis. A retained-geometry select, by comparison, must use its new bound selected rule for actual drop policy. This needs both tests; one cannot substitute for the other.

This is a required migration dependency, not a claim that the written plan explicitly proposes deleting the guard. The plan already says exact structural identity remains checked; the risk is that its named primary resolver is only one consumer of the deleted proof.

### 2. P1: The retained path must preserve the real click's dual source lifetimes

Plan reference: dependency test item 1, line 57. Allowing a nonprojecting Armed state is correct and necessary.

The current physical click path first captures the actually shown plan in `prepare_tab_gesture` (`engine/pointer_transaction.rs:3569`), including source layout facts and press-bound geometry (`:3698-3731`). It then applies selection and publishes the workspace. Afterward it recaptures the drag payload from the selected workspace (`:3963`), prepares drag authority and arms the session (`:4020-4057`). These are deliberately different lifetimes:

- Press geometry and the initiating presentation belong to the output actually hit.
- The drag payload and semantic source preconditions reflect the post-selection state.

When the threshold is crossed, `reduce_journal_pointer_move` continues from the owner-held Armed session (`:1085`), freezes presentation reservations from that press-bound geometry (`:1200-1288`) and enters dragging. A release before the threshold cancels Armed with `ReleasedBeforeDrag` (`:1366`); the terminal cancellation does not itself require recompiling unchanged geometry (`pointer_session.rs:405`).

Required implementation/acceptance:

- Do not gate retention on global `InteractionStatus::Idle`, `active_surface().is_none()`, or absence of every gesture. Those predicates would reject the benchmark's press frame.
- Nonprojecting Armed must preserve the local Select effect; starting actual drag, changed projection, source/native freeze, policy revocation or incompatible subsequent edit clears it.
- Sharing geometry does not overwrite the press's frozen presentation/source geometry with the newly minted candidate output. Conversely, the armed payload must not retain pre-selection command fingerprints merely because its geometry was shared.
- The release frame should remain on the ordinary retained Ready route unless a real dependency changed. It should not force a second full compile solely because Armed became Idle.

Use an *inactive* tab, not only reselection of the current tab. The existing `real_host_frame_tab_press_and_release_have_bounded_clone_work` starts with the selected tab (`engine/tests/host_authority.rs:954-990`), so it does not prove this changed-selection path. Assert press yields Armed, selected/MRU and rendered content change, release cancels only the arm, and a separate moved-beyond-threshold continuation still activates the correct payload. Capture compiler/hit/semantic/link counts separately for press and release.

### 3. P1: Root title selection is another mutable capture inside the otherwise immutable program

Plan reference: immutable canonical program at line 33; selected field removal at line 35.

`SurfaceProgramRootDraft.title_item` chooses the first nonempty selected pane at `scene/program.rs:465-474`, and `LinkedSurfaceProgramRoot` stores that item in the linked tape (`:165,608`). Runtime root painting copies the captured value into `SurfaceRootPaintRecord` (`runtime/paint.rs:1773`). Sharing this tape after selection in that first pane leaves an old title item even if every geometry value is identical.

The parent independently identified this issue during the review and has already chosen the correct amendment: store the stable title pane/node recipe in the program and read its selected item through the plan's immutable source binding. The first eligible nonempty title pane is stable under the selected slice's unchanged membership/order. This closes the dependency without reconstructing the program or searching every pane on each step.

Keep a direct root-title counterexample even though the initial slice excludes contained composition. Root title is public bound output, and the product benchmark clicks the first pane, which is the most likely title recipe. Do not assume that absence of a visible main-root title makes a stale title field harmless.

The parent's intended removal of linked-program construction bookkeeping (`root_drafts`, `root_indices`, deferred vectors) is an appropriate closed-finalization cleanup. It reduces retained memory and does not need a new abstraction. It is separate from the correctness fix above.

### 4. P2: Compare measurement values without treating an old ticket as reusable current provenance

Plan references: full Q and actual minima at lines 59-60; current binding at line 41.

`SurfaceMeasurements` derives `PartialEq` over its ticket and all answer maps (`scene_manifest.rs:1109`). A changed Select advances the requirement/measurement provenance. Therefore raw `old_measurements == current_measurements` will fail even when every actual geometry input matches. Replacing the comparison by an unchecked ticket rewrite has the opposite problem if it silently authorizes an external malformed or stale batch.

Use a named, owner-private comparison of the complete answer relation: keys/membership, scalar values, optional values and availability. Bind the current ticket only after its contribution is validated against the current requirement inventory. Keep this narrow comparison distinct from external stale-ticket admission. The plan does not require replacing the public detached protocol in this slice, so both responsibilities still exist.

Test that identical values with the new legitimate ticket retain facts, while a detached old ticket with those same values is rejected. Also test unavailable, invalid, missing/extra key and unrelated changed-metric cases under the surviving public protocol.

The complete per-item answers can remain identical while the effective selected minimum changes: item A and item B were both measured previously. The plan's explicit outer-minimum comparison is therefore necessary and correctly scoped. Preserve it even after proving complete answer equality.

### 5. P2: Equal scroll geometry must still adopt the new resolved selection

Plan references: dependency test item 5 at line 61 and selected owner effect at line 49.

The compiler computes `selection_changed` using `TabStripState.resolved_selection`, then reveals/clamps at `scene_compiler/surface.rs:1297-1315`. After scene installation, the reducer adopts `(bar, scroll, selected)` into the strip store (`engine/presentation_authority.rs:1219-1264`). A retained branch that copies old geometry and skips this semantic adoption leaves the strip store reporting the previous selection. Later scroll/reveal can then treat an unchanged selection as newly changed and unexpectedly reveal it again.

Update the changed bar's resolved selection even when its resolved numeric scroll offset is identical. Do not mutate the old output's source binding to achieve this. Test selection with unchanged scroll, a subsequent manual strip scroll, and another frame with no new selection. The no-new-selection frame must preserve manual scrolling rather than repeat a stale reveal.

The current reducer builds a surface-wide `selected_by_pane` map and resolved-offset vector. These are not required to disappear for the initial geometry-retention claim, but migrating selected reads must not merely replace them with a repeated linear semantic-ID search. Direct lookup through the bound node identity is enough for this design.

### 6. P2: A bound workspace snapshot introduces a real retained-memory cost

The plan correctly requires old outputs to keep their own immutable source basis. With the current whole-workspace transaction clone, binding an owning `Arc<Workspace>` into each plan extends complete old workspace lifetimes. Sharing one geometry owner therefore does not establish that total retained memory decreased.

Measure the workspace/index/source-proof storage retained by distinct referenced workspace publications alongside current/fallback/in-flight facts and measurement maps. Multiple emissions of the same publication can share one workspace; the cost is associated with distinct retained versions, not necessarily every output. Do not replace the owning old basis with a weak reference or latest-engine lookup merely to avoid counting it: that would lose the old output's selected semantics. The plan already requests memory disclosure; this identifies the specific new cost that disclosure must include.

## Dependency case: what is already sound in the plan

The bounded branch can share the complete current fact block when the owner proves one canonical-preserving Select, unchanged topology/order/ownership, current configuration and policy snapshot, matching full measurements, matching changed bar *effective* policy, matching actual effective outer minimum, and identical resulting reveal/clamp/control geometry. These are actual compiler inputs.

Specific supported details:

- `PaneRecord.selected` and `TabRecord.selected` are display semantics, while rectangles/hit regions depend on effective minima, policy and reveal. Removing those two captures plus the root title capture permits geometry reuse.
- The semantic manifest stores receiver kinds and derives close/control/splitter membership from geometry/policy, not tab selected flags (`semantic_manifest.rs:28-115`). With no popup and unchanged controls/close facts, its receiver storage is reusable; its output wrapper still changes.
- Magnetic fields currently hold physical bounds, target IDs, depth and config-derived bands (`target_field.rs:13-80`). With fixed structure/minima/config/scroll and unchanged guides/targets, they do not require a new selected-state copy.
- Pointer/touch fragments can be reused without replay only when their complete geometry/protected-chrome context is unchanged. Falling back when reveal changes receiver geometry is correct for the first slice.
- Equal effective bar policy does not establish drop policy equality because `pane_local_target_rule` reads the currently selected item (`workspace.rs:730`). The plan correctly keeps live drop policy outside geometry reuse.
- Structural workspace cloning and fresh semantic source capture can remain. They limit the speedup; they do not invalidate the avoided-construction result. Rewriting the arena or public frame Interface is not necessary for this experiment.

Do not widen the branch based on equal visible rectangles alone: split child minima/maximums, resize operability, title semantics, root owner, coordinate/native provenance, controls, and actual command policy are also observable. The listed full-input guard and source-binding checks are substantially stronger than rectangle equality.

## Physical workload and the 10% acceptance target

The unchanged complete product benchmark is favorable to the bounded branch for concrete reasons, but this is still a testable hypothesis:

- `integration/egui-product-harness/examples/product_cost.rs:20-24` uses a 16,384 by 16,384 surface. Its balanced 1,024-leaf layout has two fixed items per leaf. It uses fixed pane minimums and default presentation policy with close disabled (`:254-269`).
- `Harness::sample` gets the actually inactive first-pane title bounds, submits a hover-only preparation, then times a press frame and a release frame (`:503-578`). Selection and actual rendered content are checked after the release. It does not time a semantic `Select` field assignment.
- With two tabs in a large stable leaf, full values, effective policy and reveal are likely unchanged, but eligibility counters must establish this in the exact benchmark. Presence of title text alone is not a proof that reveal geometry is unchanged.

At the cited historical complete click baseline of 25.397584 ms, the working goal requires at least 2.5397584 ms less work, or a candidate p50 at most 22.8578256 ms, using an actually comparable qualified baseline. The current source samples observed scene compiler, validator and requirement-refresh work during click. Those categories overlap, include untimed preparation/warmup, and are inclusive samples rather than a timed-frame partition. Their percentages cannot be added or used to promise the required saving.

The selected scope removes a meaningful cluster: selected-path full requirement derivation, complete surface record compilation/finalization, region replay/touch construction where present, semantic reconstruction and paint relinking. It does not remove current complete measurement callbacks and map assembly, workspace/source-proof work, widget/child UI creation, complete content paint, output settlement or tessellation. That is a plausible investment case for 10%, not evidence of attainment. A broad new arena/public Interface would add risk without first answering this narrower question.

Before formal timing, establish in untimed/focused runs:

1. The real changed-tab press selects the retained branch at all three existing sizes, including 1,024, and no changed-minimum/policy/scroll guard accidentally forces fallback.
2. Press has zero complete compiler, validator-as-constructor, hit-region emission, touch compile, semantic roster build and paint-link build calls. Release has no corresponding unnecessary construction. Keep measurement and paint/content counts identical.
3. Surface requirement/index/strip reconciliation work for unrelated leaves actually disappears, rather than moving into a helper with a different name. Refreshing current root semantic proof once is acceptable if disclosed and counted.
4. Record the remaining full-root fingerprint/source captures, transaction copies, full measurement comparisons, publication equality scans and retained scene clones separately. If timing misses the target, these data tell whether the path was ineligible or the remaining work dominates.

Use test-only counters or a deliberately nonqualifying attribution build for those assertions. Then time the clean exact-final binaries in the two independent serial comparison groups. The final complete-workload timing is the acceptance result; the counters explain the mechanism. A fast semantic headless selection alone cannot qualify the physical click target.

If a substantial correctly eligible retained branch falls short of 10%, report that result rather than expand the eligibility predicates or weaken correctness. Inspect the remaining attributed cost and choose a bounded next change. The working target does not authorize unbounded generic proof machinery or require replacing all in-process modules.

## Minimal plan amendments

Add these concrete obligations without changing the selected architecture:

1. Name `LinkedSurfaceProgramRoot.title_item` in the selected-state deletion list; replace it with the parent's stable title-pane recipe.
2. Name `presented_root_structure_is_current` and all consumers as source-basis migrations required when deleting target destinations.
3. Specify measurement answer equality separately from current contribution-ticket validation.
4. Require adoption of new resolved selection on an equal-scroll retained bar.
5. Require the inactive-tab physical press/Armed/release counters, plus the subsequent threshold-activation counterexample, before timing qualification.

With those obligations, the plan is sufficiently concrete to implement and independently review. The most useful next review surface is the actual representation/source-binding diff and the exact retained-path guard; no further general design framework is needed.
