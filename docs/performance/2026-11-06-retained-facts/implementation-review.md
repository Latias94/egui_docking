# Independent retained-facts implementation review

Date: 2026-11-06. Baseline: `34ab23a`. This review inspected the actual in-progress working tree against `docs/plans/2026-11-06-001-dockspace-retained-facts-refactor.md`.

Scope: correctness, source/output identity, actual avoided construction, pointer press/Arm, retained measurements, native lifecycle, and rollback. No repository edit, Cargo command, test, benchmark, or Git write was performed. The only write is this report. Existing user-owned workflow/review changes are outside review scope.

The coordinating agent continued editing while this review ran. The first principal source receipt was at `2026-11-06T11:32:06Z`; a further targeted review completed at approximately `2026-11-06T11:36:46Z` after the real press/release test was added. Line numbers identify the inspected shape, not a later immutable commit. Newly added minima, offscreen-reveal, unrelated-answer, and physical-pointer tests were inspected when they appeared and are credited below. No test result or pilot timing is independently claimed by this reviewer.

## Judgment

The implementation makes the essential split real. The retained path shares the exact `SurfaceFacts` owner, root layout storage, hit/touch geometry, semantic receiver storage, and paint program link. It does not rebuild every record to discover equality. Current selection, command root basis, measurement provenance, output ticket, and concrete emission authority stay outside that shared geometry owner.

I found no confirmed P0/P1 authority, rollback, or native-lifecycle regression in the paths inspected. There is one concrete numerical-equivalence issue in the new reveal predicate that should be resolved or falsified with a focused regression before declaring the conservative and retained paths equivalent. The newly added actual pointer press/release test now covers that common sequence; threshold activation, historical-output rejection and reuse-specific restart still need focused assertions.

## Finding: P2 — the retained reveal predicate reconstructs different inputs from the complete compiler

Location: `crates/dockspace/src/scene_compiler/surface.rs:2092` in `selection_geometry_unchanged`; compare `:1280-1293` in `compile_tab_strip`.

The complete compiler derives the selected range from the original allocated widths:

```rust
let start = widths[..index].iter().sum::<f64>();
Some((start, start + widths[index]))
```

The new predicate derives it back from translated, already rounded rectangle endpoints:

```rust
let start = member.full_bounds().x() - record.viewport().x() + record.scroll_offset();
let end = start + member.full_bounds().width();
```

`LogicalRect` stores min/max corners, and `width()` subtracts them (`geometry.rs:152-229`). Absolute placement can therefore lose both prefix and width precision. The two formulas are not equivalent, even when every measurement answer is identical. Using the same `reveal_tab_range` helper afterwards does not restore that lost information.

A finite arithmetic counterexample is a strip viewport near `x = 1e16`, with its represented width `288`, three allocated widths of `97`, and old scroll `0`. At that magnitude the spacing is `2`; successive translated `cursor += 97` operations can each advance by `96`, and a stored full rectangle's width becomes `96`. The retained third member can then reconstruct range `[192, 288]` and return old scroll `0`. The complete compiler's range is `[194, 291]`, maximum scroll is `291 - 288 = 3`, and its new-selection solve returns `3`. Those are all finite positive values. Suitable chrome reservations/configuration can be chosen so this is the represented content viewport; the issue is not specific to invalid NaN/infinite geometry. Smaller rounding discrepancies also occur with fractional widths/origins.

This counterexample was derived from the actual arithmetic, not executed as a test. The precise full builder fixture still needs to be constructed and checked against its production geometry validator. The source-level inconsistency is definite; the full end-to-end reproduction remains unrun.

Consequence: `selection_geometry_unchanged` can return true even when the sole complete builder would change scroll, control enablement and translated receiver geometry. This violates the plan's dependency-equivalence claim and can also introduce unnecessary conservative misses in the other rounding direction. It should not be hidden with an epsilon equality test.

Recommended fix: retain the exact allocated local range/width inputs used by compilation, or factor the changed-bar range/allocation computation so the conservative builder and retained predicate use identical inputs and arithmetic. Recomputing only the changed bar from retained measurements is also bounded and acceptable; replaying the whole surface is not needed. Another valid resolution is to deliberately define one shared canonical range calculation for both paths, with its behavior documented and verified. A broad floating-point/cache framework is unnecessary.

Required regression: a valid nonzero-origin/fractional or large-finite-origin overflow strip; select across the reveal boundary with identical Q, compare retained eligibility and final scroll/control/member/hit facts with the conservative builder. Exercise both an actual change and a truly unchanged reveal. The ordinary offscreen test added during review covers the normal case but not this numerical reconstruction issue.

## Source and binding audit

### Retained facts and current selection

`PresentationPlan` separates `facts: Arc<SurfaceFacts>` from provenance, measurement/layout inputs, `selection`, and `command_basis` (`scene.rs:205-263`). Pane/tab geometry no longer stores selected fields. Runtime paint views obtain current selected values through `pane_selected` / `tab_selected`; the underlying selection is a frozen map owned by that plan, not the mutable current workspace.

`rebind_selection` (`scene.rs:406-427`) clones the small plan header and shared owners, changes one selection entry, replaces the affected root basis and measurement snapshot, and does not call `facts_mut`. Therefore an old output keeps its old selected item and command basis. `PresentationLayoutFacts::rebind_measurements` shares the root layout map and replaces only the actual measurement snapshot (`scene/records/layout.rs:83`). Because eligibility first requires all answer values/keys to match, its retained per-root minima and current measurement answers cannot diverge on this path.

The linked root program changed its title source from an ItemId value to a tabs identity. Visiting it reads that plan's selection (`scene/program.rs:297-305`, `:819`), so a reused program does not keep the old native/main title item. The new direct-Select test checks this behavior.

### Target old basis to live target

`DropTargetLocation` contains structural intent and fraction but no command proof. `check_target` reads the exact target plan's root basis (`drop_resolver.rs:874-881`), captures the same live typed target, checks surface identity, and compares `presentation_structure_eq` (`:992-1053`). Selection/MRU may change; root/node/ownership/order/split-weight changes cannot silently retarget an old location. Missing basis fails closed.

Actual target policy is still evaluated through `DropCommandEligibility::check_target_indexed` (`drop_resolver.rs:895`) and transaction preflight. The selected-item-derived rule is recaptured from the live workspace rather than embedded in the geometry. This is the right ownership seam. Equal tab-bar policy is not being used to authorize a drop.

Production full compilation binds the main and every contained root basis once (`scene_compiler/surface.rs:134-135`, `:178-179`). Synthetic standalone validation clears/rebinds its basis map from the supplied workspace (`scene/validation.rs:191-209`). Production finalization now trusts its own closed constructor's already bound selection/bases instead of performing another selection walk. This update appeared during review and avoids the earlier duplicate binding traversal.

The target validator still checks real surface/root/node membership, gap range, edge kind/root-node identity, layer, geometry, canonical guides and coverage. Deleting ID-versus-destination comparisons is appropriate because the new type no longer represents those inconsistent pairs.

### Eligibility and actual pointer Arm

`advance_selection_revision` retickets the affected surface, rebinds its structural index and changed root semantic snapshot, reconciles scene authority, and records at most one reuse preparation (`engine/presentation_authority.rs:204-266`). The candidate source must be a confirmed current Ready output; an older fallback under an unpresented candidate is not used. No popup, contained composition, native create or recovery case becomes eligible accidentally. `after_selection` recomputes the changed leaf's complete effective policy and item close capabilities (`scene_manifest.rs:875-966`).

The actual pointer path was updated, not only the public semantic Select path: `activate_prepared_journal_tab_gesture` publishes an exact single Select then calls `advance_selection_revision_caused` (`pointer_transaction.rs:3909-3916`). It recaptures the payload from the post-selection workspace (`:3967-3971`) and arms with `source_validated_at: self.version` (`:4035-4053`). The contribution path explicitly permits Armed and refuses Dragging/Resizing/ContainedTransforming; it also checks actual projection inputs (`surface_contribution.rs:346-385`). This source trace supports the intended real-click eligibility.

The new `changed_tab_press_and_release_retain_measured_and_presented_surface_facts` test in `engine/tests/host_authority.rs` stages a physical pointer journal and a real receiver delivery from the old Presented output. It checks post-selection MRU, Armed payload equality with the post-selection source, retained old source-measurement ticket, complete program content selection/title, fresh output ticket, shared fact/hit/semantic/link owners, release cancellation before dragging, zero full compiler count on both edges, and eventual Presented promotion. That addresses the earlier basic pointer validation gap. It does not cross the threshold to activate a drag, or attempt an unpresented successor/retired old-output misuse.

General revision/requirement refresh clears the preparation before publishing incompatible state. A second selection that no longer has a confirmed Ready source conservatively loses whole-fact reuse. The manifest/index-only rebind can still be correct in that case; the geometry optimization is not promised for multiple edits.

## Measurement, minima, and scroll audit

`SurfaceMeasurements::same_values` compares all five actual value/key domains, ignoring only the ticket. It cannot silently skip a changed hidden-item minimum or changed intrinsic. The runtime measurement loop remains unchanged and still calls the resolver for complete Q before preparation. Exact detached submission and unavailable paths remain the ordinary compiler's responsibility when equality fails.

The extra effective-selected-minimum test is necessary and present: `selection_geometry_unchanged` compares old/new item measurements through the same `pane_outer_minimum` used by root measurement, including configured minimum and tab-bar height (`surface.rs:2067-2078`). `RootLayoutFacts` includes every requested item minimum, not just the active item (`surface.rs:1983-1997`). A changed active constraint therefore falls back even when the entire answer set is unchanged.

The reveal calculation consults current requested scroll and resolved selection, then checks the resulting offset against the retained record. If the offset actually changes it takes the full path, so pointer/semantic/control/touch dependencies are rebuilt together. The numerical issue above is the remaining concern; there is no separate unsafe assumption that equal bar bounds imply equal touch geometry.

Tests added during this review now cover:

* changed selection with the same facts and fresh semantics/output;
* unchanged answers with different actual selected minima;
* offscreen selection requiring reveal and new hit geometry;
* selection plus another item's changed minimum/intrinsic;
* actual pointer press/Arm/release with complete program recording and final Presented promotion.

Those tests have meaningful observable assertions beyond a reuse flag. Their execution is the coordinating agent's responsibility; this review did not run them.

## Historical outputs, native lifecycle, and rollback

`install_ready` mints the fresh scene/output ticket before the shared-facts branch and constructs new output wrappers with that ticket (`scene/lifecycle.rs:1255-1279`). It shares only the hit body, semantic receiver body and paint link. It then retains the old candidate/fallback and its previous authority separately. A shared facts pointer never itself creates `PresentedSurfaceAuthority`.

The ledger/settlement route remains responsible for stream, endpoint, emission and coordinate generation. Existing old-output eligibility checks remain in place. `presented_root_structure_is_current` now reads the bound root basis directly and still checks topology, placement and actual/override split weights. Pane receivers compare the shown plan's selected item with the current workspace, preventing old selected-content identity from silently becoming new content.

Native first-live compilation uses the prospective workspace/index supplied to its closed compiler; it does not bind its command basis from the live owner. Selection's new retained branch excludes any native-create saga and all active recovery maps. The earlier exact full frozen source checks, including selection, remain before workspace publication (`engine.rs:4190-4225`). Thus geometry sharing does not authorize a native ownership transfer. I found no code path in this diff that removes those gates.

The further native trace confirms this in both directions. While any create saga is live, `advance_selection_revision` takes the ordinary full requirement rebuild, including prospective children; an unrelated selection's version change therefore refreshes the first-live projection stamp instead of leaving a stale prospective ticket authoritative. Selecting the frozen source root is rejected by the unchanged exact source barrier before this revision path. `native_first_live_projection` stages the original command under its saga authority and creates a prospective workspace/index at the next revision (`engine/native_projection.rs:265-312`). Its freshness check still requires the live base version, config/policy, ready basis, exact saga binding/resource/phase and Pending admission (`:233-260`). Commit additionally requires the exact candidate ticket and owned native-create barrier (`:102-117`), executes a general revision path that clears `selection_rebind`, then accepts that specific observed output. Direct prospective compilation does not consume a leftover selection preparation.

The same-bar/different-drop-rule trace is also intact: `after_selection` uses complete bar-policy equality only to retain its question roster; `WorkspaceIndex::capture_tab_target` and `capture_inner_edge_target` call `workspace.pane_local_target_rule` on the actual current selection (`workspace.rs:390-433`). `DropCommandEligibility` validates the resulting target and calls `authorize_drop`/the current policy evaluator (`operation.rs:959-994`). No stored geometry availability or old command basis is used to bypass that decision. A targeted retained-output test is still appropriate because the migrated contract test checks the new rule value after resealing rather than an actual denial after retained installation.

The encoding checkpoint clones all `PresentationAuthorityState`, including `selection_rebind`; restart restores it and preserves revision/output allocation frontiers (`host_frame.rs:1040-1078`). New serial allocation still uses the nonrollbackable shared issuer. A dropped frame discards its candidate. These mechanisms appear sufficient, but the new combination needs the behavioral tests below because the preparation is cleared after an installed contribution and must reappear after restart.

## Targeted test requirements before qualification

These are concrete validation gaps, not claims that all of the following are broken. Prefer adding assertions to existing interaction/host fixtures instead of building new protocol harnesses.

1. **Threshold activation after a retained press.** The newly delivered real press/Arm/release test covers the ordinary click sequence and its source provenance. Add a sibling case that crosses the threshold while holding after the retained output has been installed: it must start the expected drag with the post-selection fingerprint, without a stale-source rejection or retargeting to an unrelated current output. Compare the armed source geometry's old output provenance with the active source semantics; the legitimate difference must remain intentional.
2. **Same facts, different binding, exact presentation.** Retain B1/E1, select and encode B2/E2 with the same facts but a new ticket. Before E2 is Presented, input claiming E2 must be rejected. Present/Drop E1 late in both orders; it must not confirm E2, replace B2 selection with B1 selection, or upgrade a stale pane receiver. Then Present E2 and show its receiver descriptors carry E2's wrapper/authority. Include final-output mutation or incomplete-pass refusal through the existing real egui/headless settlement seam.
3. **Restart/drop with reuse preparation already consumed.** Select in a frame with a valid retained source, capture encoding checkpoint, prepare/install a reused contribution, restart, then encode again. The second encoding must either reuse the restored valid preparation or conservatively compile; it must never reuse the first attempt's output capability. Drop that frame and verify original selected/MRU/source/output/fallback remain unchanged. A successful retry needs a fresh emission/output allocation where required, with no ABA.
4. **Equal bar policy, different drop rule.** Use two items whose effective tab-bar policies are equal but whose target drop rules differ. Select into the retained path, then attempt center/edge drop from an actual presented output. The new item rule must decide acceptance. The migrated contract test checks the new target rule value after seal, but does not exercise retained installation plus an actual policy rejection.
5. **Native freeze and unrelated-surface isolation.** With a native first-live source freeze pending, selecting that source remains rejected atomically. A selection on another ordinary surface may proceed but must not bypass the native prospective provenance/observation gates. Include coordinate reattach/retirement between preparation and contribution; the old token or output must fail even if the same `SurfaceFacts` is reachable.

The minima/offscreen/unrelated-Q/basic physical press-release cases are no longer missing in the working tree observed near the end of this review. The numerical reveal regression remains a specific addition to those tests. The five items above describe the remaining specific combinations, not a demand to rerun or duplicate all existing lifecycle tests.

## Actual sharing versus remaining work

The geometry promise is fulfilled in code: `rebind_selection` never calls `facts_mut`, retained root layouts use Arc, and `install_ready` directly calls `rebind_output` rather than hit/touch emission, semantic compilation/sort or paint relinking. The new pointer-identity checks for facts/receivers/link and zero compiler count are consistent with those source paths. `HitGeometry` contains both ordinary rows and touch fragments, so sharing its owner preserves the first subsequent touch's exact frozen geometry without a deferred rebuild.

Do not extend that claim to zero structural copying. `WorkspaceIndex::after_selection` clones its root directory and `Arc::make_mut` clones the selected root's fingerprint node vector (`workspace.rs:201-209`); the arrays inside those records are shared, but the vector still has root-sized copied volume. It also scans that vector to find the changed tabs node. `PresentationPlan::rebind_selection` copy-on-writes a `SecondaryMap` containing all workspace selections (`scene.rs:424`). The general workspace transaction clone is intentionally retained. Existing fingerprint-build counters do not account for these direct clone volumes, so a zero counter must not be described as constant-time source/state rebinding.

This is an attribution/qualification limitation, not a request to introduce a persistent graph before measuring the chosen slice. The selected plan explicitly permits the general transaction arena. Count or disclose remaining copied state volume, and let serial complete-workload timing decide whether further storage work is worthwhile.

The Module now has materially more Depth at the existing frame Interface: the Adapter still measures and paints complete output, while the Implementation decides whether already sealed geometry survives a source edit. The useful Seam is facts versus current binding; no extra rendering Adapter protocol or generic dependency engine was introduced. Its Locality is strongest for the ordinary selection case, with unknown geometry-producing cases using the existing sole complete builder.

## Source receipt

Read-only hashes from the late review snapshot (`2026-11-06T11:32:06Z`):

```text
scene.rs                                      7c40098ff4d47cd18352a67c488fb64ddb1447d2bae50b2734b43510c48593d1
engine/presentation_authority.rs               ea6a3491baeb946cc51da1737191c190300a5d675ff579cea7a407c680552026
engine/surface_contribution.rs                 eaa91a5572f9e0affa1a24953a314bd34ee21f941e7aecb8e32141bf079e1ac6
engine/pointer_transaction.rs                  aff075e2f848dddcdd1fac26dae06f00541fd13fcb672ea339a0f1728e1da86d
scene_compiler/surface.rs                      57dad8f1f9c0d843598b7e096a533453687581dcdd996987a82def7f18a30c84
scene/lifecycle.rs                            45437439078bc8da8eabb3877d360bdf7171dbbb3ee499ed38055a951b43a88a
engine/surface_contribution/reuse_tests.rs      51aa48fdf82e6d8045e3c93c006b71bb33c43bd9dc6b47c97907c73d13070972
```

The report contains one P2 numerical-equivalence finding and five concrete retained-path test requirements. No generic style finding, speedup claim, compiled correctness claim, or observed test pass is included.

## P2 fix review — source issue closed, numerical regression pending

Follow-up source review: `2026-11-06T11:44:37Z`. No source changes, Cargo commands, tests, or benchmarks were performed by this reviewer. This section supersedes the open-source disposition of the P2 finding above; the earlier finding remains as the review history and explains why the change is necessary.

The new Implementation addresses the identified cause directly:

* `TabStripMemberRecord` stores a private `allocated_width: f64` and exposes only a crate-private getter (`scene/records/tab.rs:462-500`).
* A repository-wide source search finds one `TabStripMemberRecord::new` call. It passes the original allocated `width` directly from the compiler's `widths` iteration (`scene_compiler/surface.rs:1316-1322`), before any conversion back from rectangle endpoints. All members, including hidden ones, preserve these values in the same item/ordinal order.
* The full compiler now calls `tab_local_range(widths.iter().copied(), index)` (`:1280-1282`).
* The retained predicate finds the selected item in the retained ordered member list and calls the same helper over `member.allocated_width()` (`:2088-2102`).
* `tab_local_range` takes the first `ordinal` values and computes `sum::<f64>()`, then adds the next original allocated width (`:2109-2112`). Both routes therefore execute the same prefix/add operations on the same ordered original values. They do not reconstruct local ranges through absolute `x`, represented rectangle widths, or old scroll.

The existing eligibility checks establish why the original allocation values may be reused: complete measurement values and keys, effective bar policy, config, topology/order, selected effective minima and viewport-producing inputs are unchanged. The helper returns `None` for an out-of-range ordinal without panic; a valid closed-construction member roster resolves the same ordinal as the compiler. No epsilon comparison, expanded proof framework, extra public constructor, or whole-surface replay was introduced.

**Disposition:** the P2's source-level numerical inconsistency is closed. I found no new correctness issue in the fix and no reason to block the coordinating agent's serial qualification on the old inverse-coordinate calculation. The valid compiler numerical fixture being prepared by the other design lane remains required validation evidence; this reviewer has not observed or executed that test yet.

Remaining risks are the already identified behavioral combinations, not additional objections to this fix: threshold activation after retained Arm, consumed-preparation encoding restart/drop, old/unpresented output disposition with shared facts, and equal bar policy with an actual different drop-rule decision. The coordinating agent has delegated short regressions for these. Native trace remains sufficient as a source audit; this follow-up does not request another general native test sweep.

The added retained width costs one `f64` per tab-strip member and participates in the existing derived value comparison. That is a concrete bounded data-layout cost, not a new cache or authority domain. Count it in retained-memory accounting if that accounting reports record sizes.

Source receipt for this fix review:

```text
scene/records/tab.rs        d23525c0140d2f8cc9cd137c42f97aad0093f2385029c7884400823ca29391c0
scene_compiler/surface.rs   e5d5500cf76d974f85f1adeee90022161ddab3f04cf9c32ad39f3a4ca6857c74
scene_compiler/tests.rs     0d3b8d6a5a9ed00a017ba96c72c7cf903d716e92c703d47694cd3e88333f3f31
```
