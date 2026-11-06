# Independent checked-move implementation review

Reviewed: 2026-11-06T14:39:44.147658+00:00. Baseline: `a2bcabc1440caa664fb5f4a64c4669c4eecfdce4`. Scope: current working diff in `operation.rs`, `workspace.rs`, new `operation/tests/checked_move.rs`, `canonical.rs` and `validation.rs`; the small `drop_resolver.rs` work-ledger assertion update was also checked. The plan is `docs/plans/2026-11-06-002-checked-move-refactor.md`.

**Verdict: no blocking correctness or design finding. Standards: pass; Spec: pass for the declared implementation slice. The current source can be frozen for final root-run checks and candidate timing.** No source edit is required by this review. This verdict does not assert that the performance investment criterion has been met.

The review read the diff and relevant callers/tests, and ran read-only diff/hash checks only. No Cargo, compiler, test, benchmark, binary, profiler, network or Git write was executed. No nested agent was created; the user's explicit review constraint overrides the code-review skill's parallel-agent workflow. User CI/review/reference changes were excluded and untouched. The only write is this temporary report.

## Standards

No documented-standard violation or actionable code-smell finding was identified in scope. The change uses existing WorkspaceIndex, payload policy facts, target capture and execution routines; it introduces no generic proof schema, authorization token, persistent cache, compatibility executor or second docking implementation. Comments and new regression fixtures remain local and readable.

`WorkspaceIndex::for_command_reference` and `include_command_reference` are crate-private helpers with exactly the current command owner as production caller. The constant initial version is an internal compatibility key for that freshly derived local index, not caller-supplied proof of freshness. Correctness follows from actual extraction and the enclosing exclusive candidate borrow. The helper itself is not a general cross-workspace cache, and its documentation correctly forbids carrying it across mutation.

The explicit early edge-scope check in `check_move` repeats a small check also present in target validation. Here it preserves the existing scope-before-root-capture error boundary while avoiding eager target materialization. It is not a reason to add a new abstraction or broaden this slice.

## Spec and adversarial checks

### The private operation owns the full check/execute interval

`operation.rs:1421` routes every actual Move through `move_payload`; `operation.rs:1578` holds the same `&mut Workspace`, calls `check_move`, and then executes that exact payload/target without any intervening callback or candidate edit. `check_move` returns only `()`: neither reusable facts nor a checked token escape. The local index is dropped before the first mutation. Both transaction preparation and owned-draft follow-ups use this dispatcher. Standalone authorization shares `check_move` but cannot bypass the execution owner's fresh check.

Every batch iteration calls this path against the **current staged candidate**. There is no transaction-start proof collection and no caller-version cache. `selection_before_move_rechecks_every_target_against_staged_state`, `selection_before_move_rechecks_all_payload_kinds`, and `earlier_move_invalidates_later_target_without_reusing_batch_facts` directly cover four target variants, all three payload variants, and rollback after an actual earlier Move.

### Source success cannot authorize another frozen target

The root index is materialized at most once per distinct referenced root, but each reference is independently checked against its own expected fingerprint. In particular, same-root source success does not skip target fingerprint equality. `current_same_root_source_does_not_accept_an_older_target_snapshot` isolates a fresh source plus old target and requires target-role rejection.

A current fingerprint alone is insufficient to authorize embedded target policy facts. Existing target validation still re-captures the actual target and compares the entire value, including surface, central status, exact selected-item/Root rule and edge node/scope. `current_fingerprint_cannot_authorize_forged_target_policy_facts` mutates each of surface/central/rule across all four target variants and requires rollback. The outer-edge recapture derives the root node, so relabeling an arbitrary member as the outer boundary still fails.

### Membership/error boundary remains explicit

`workspace.rs:99` checks root existence, arena node existence, and (for a not-yet-materialized root) actual membership before deriving the root snapshot. This preserves NodeOutsideRoot ahead of unrelated malformed-root extraction failures for the first reference. When the same root is already indexed, `capture_reference` still checks membership in that root's frozen members set. The workspace cannot change during either path.

`scoped_facts_reject_foreign_nodes_for_both_reference_roles` covers valid foreign Source and Target nodes. Expected errors are typed and exact rollback is checked. No evidence was found of a source/target membership bypass, or of a same-root different-proof acceptance.

### Actual policy still precedes no-op/mutation dispatch

The ordinary dispatcher checks the arrangement lock first. `check_move` then validates payload authority, target authority/scope, move semantics and actual payload/target policy before any same-pane or equivalent-edge no-op return. It reuses the existing `DropCommandEligibility` policy evaluation, including all payload items, source owner/presentation, undocking classification, exact target surface/rule/central status and the special effective tab-bar gate for TabGap.

The new `same_tabs_noop_move_still_checks_tab_gap_policy` closes the earlier direct Move/TabGap coverage gap using a valid no-op gap with interaction disabled. Existing public tests continue to cover item/root/surface source gates, all-item Tabs/Subtree class checks, selected target invalidation, inner-versus-outer rule scope, empty-central Root policy and central restrictions. The removed execution checks are covered by the common precheck; policy has not been replaced by cached UI bar facts.

### Detachment does not reuse obsolete mutation operands

The change reuses authority extraction only. It does not precompute and preserve parent indices or weights across detachment. Existing insertion routines still find the target after extraction. `moving_sibling_tabs_uses_parent_weights_after_detach` is a concrete behavior oracle: `[A,B,C]` with weights `[0.2,0.3,0.5]` becomes `[B,A,C]` with approximately `[0.1875,0.1875,0.625]`. It checks actual output rather than mirroring the retention algorithm.

### Policy-independent recovery remains narrowly authorized

The only new execution caller supplying `None` policy is `TransactionAuthority::SurfaceRecovery`. Preparation still requires `SurfaceRecoveryTransaction::is_exact_program()` and the exact current frozen source roster. The type's private constructors constrain it to a complete recovery program and destination surface. Ordinary commands copied from the program continue to use ordinary policy.

`check_move(..., None, ...)` skips only policy evaluation. It still verifies source/target proofs, membership, target derivation, payload/target shape, descendant restrictions, central preservation and edge-insertion feasibility. Some structural failures now occur before extraction instead of later during insertion/detachment. This does not admit an invalid success, and accidental precedence between simultaneous invalid causes is not an acceptance contract. Exact recovery Move programs carry the whole main root to another surface, so the new preflight does not acquire the same-root detach/weight hazard.

The added `exact_main_move_recovery_bypasses_policy_but_rechecks_its_target` uses the real typed recovery compiler for both Tabs and an actual split Subtree. It checks ordinary policy rejection, stale-target rejection with rollback, and successful policy-independent typed recovery with item conservation and strict validation. No new recovery authorization bypass was found.

## Canonicalization and validator borrowing

`canonical.rs:274` borrows the current node instead of cloning it on both Enter and Exit. The traversal still copies child identities into the explicit reverse-order stack and processes children before their parent. Input cycle/shared-node/weight/selection checks remain unchanged.

`rewrite_split` first reads child rewrite results and same-axis status. Zero/one retained child returns the same result as before. An unchanged split returns early only when neither child replacement/removal nor same-axis flattening is needed; its existing weights/storage are retained bit-for-bit. An actual rewrite borrows source child/weight slices, composes the same parent×child weights in the same order, normalizes them once, and writes owned replacement vectors only after the read borrow ends. Child rewrites remain visible at the parent Exit, and arena sweeping still happens after every root rewrite. No orphan/item-conservation/central behavior change was found.

Existing `canonical_graph.rs` cases cover nested same-axis composition/fixed points, empty leaves/singletons with central preservation, promoted empty central identity, orphan rejection, malformed graphs and deep iterative traversal. Existing private tests retain atomic wrapper rollback and late-output error behavior. No additional generic clone instrumentation is needed to establish the source change.

`validation.rs:534` now copies the existing immutable workspace reference and borrows the node while mutating only validator bookkeeping. It visits the same reverse-pushed children and invokes the same tab/split checks in the same order. The new `validation_preserves_preorder_and_tab_diagnostic_order` exercises same-axis nesting, bad weights, selection/MRU diagnostics, duplicate items and shared-node reporting in one ordered result. No skipped validation or changed diagnostic ordering was identified.

## Work reduction and validation limits

The implementation actually removes repeated root **construction**, rather than merely adding pointer equality. The command-local BTreeMap stores only referenced roots; no full workspace index build is added. `one_move_derives_only_its_distinct_current_roots` requires one build/three visits for same-root moves and two builds/six visits for cross-root moves across four targets in its two-leaf fixtures. The resolver fixture's source has three nodes and target one, so the changed expectation of **2 builds / 4 visits**, replacing 6 / 10, matches the new extraction scope.

The one full transaction candidate, item ledger and reconciliations, final canonicalization and strict validation are intentionally retained. No claim is made that all remaining scans, equality comparisons or allocations are gone.

I inspected the completed first all-features log: **2,019 run, 2,019 passed, 3 skipped**. The recovery and foreign-node tests were added afterwards; their final root-run validation was still compiling at this review snapshot. I did not rerun either suite. The earlier default-core fail-fast work-counter expectation was obsolete, and its exact updated formula is consistent with the source; this review does not substitute the earlier partial run for final verification. Read-only `git diff --check` was clean for the four changed tracked implementation files.

## Deferred target equality proposal

Keeping the current full `actual == target` comparison for this freeze is appropriate. After successful `verify_reference` and capture against the same immutable candidate/index, the repeated full fingerprint comparison is logically redundant. A future small change could compare TabTarget's independently derived surface/central/rule, and EdgeTarget's corresponding fields plus derived node (essential for OuterEdge), while retaining the earlier scope and reference checks. Root/edge/fraction are currently supplied from the same arguments.

That proposal is optional performance work, not a correctness fix. A hand-maintained field list would need to stay synchronized with future target fields and retain forged-field/outer-node tests. The present slice already removes root rebuilds and per-Node clones without changing that equality contract. There is no strong reason to expand it during final validation.

## Reviewed source snapshot

| Path | SHA-256 |
| --- | --- |
| `crates/dockspace/src/operation.rs` | `61f4ddde1d2bec8fae050cc455c1cf45161ad0949b56a74b85e95ebc117aeda9` |
| `crates/dockspace/src/workspace.rs` | `941048a4a3f1905fa66b6f335fc04a6949afd3c8bffcd9c3e8da6526f3263128` |
| `crates/dockspace/src/operation/tests/checked_move.rs` | `fe42f130f328253444a7d29bec738f58f0058a59f19afebd9be14195d04013dc` |
| `crates/dockspace/src/canonical.rs` | `baf05788203163e013824c5ebf6de3ec9bae40b902e84d09d3c7f0e8c6a97c5c` |
| `crates/dockspace/src/validation.rs` | `1e71cd27d8932e98e5ab56260d492242fa24f815cac13e2add73efdaaceb80bf` |
| `crates/dockspace/src/drop_resolver.rs` | `8b0af79ba94b039347db24cca17df6365b7793039c88559f61f046429b4b15e0` |

Final finding count: **Standards 0; Spec/correctness 0 blocking or required findings.** Freeze the current candidate and complete root-owned final checks and measured qualification. Any final performance claim remains contingent on the planned raw/full-state comparison and both serial groups, not on this source review.
