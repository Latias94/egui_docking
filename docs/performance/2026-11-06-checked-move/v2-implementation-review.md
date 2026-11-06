# Checked-move v2 incremental implementation review

Reviewed: 2026-11-06T14:56:31.002679+00:00. Baseline for this review is the independently reviewed and measured **v1**, not repository HEAD. The seven copies in `v1-sources/` were each independently hashed and matched the exact corresponding `candidate-reference/environment.json` source entry before comparison.

**Verdict: no blocking or required implementation finding. The bounded v2 increment preserves valid/invalid behavior, diagnostic order and iterative traversal tolerance. It is suitable for freezing after the root's final checks.** Standards: pass. Spec/correctness: pass for this increment. Performance qualification remains separate; the failed first-iteration 20% gate is not changed by this review.

Only `graph.rs`, `validation.rs`, `workspace.rs` and `operation.rs` differ from those v1 copies. `canonical.rs`, `drop_resolver.rs` and `operation/tests/checked_move.rs` match v1 exactly. This review read the incremental source and relevant consumers/tests and performed hash/diff checks only. No source edit, Cargo, compiler, test, benchmark, executable, profiler, Git write, network or nested agent was performed. The only write is this report. Root-owned all-features verification was still compiling at the inspected log snapshot; no passing v2 suite is claimed here.

## Exact short MRU predicate

`graph.rs:15` retains the same common conditions: MRU must exist, its length must equal the visual item length, and its first member must equal selected. These checks now run before allocating the fallback sets. Their order has no observable semantic effect because this helper returns only a boolean and mutates no state.

The 0/1/2 cases are equivalent to the previous unique-MRU-set/visual-set equality test:

- Zero items implies zero MRU entries, and the selected-first condition already requires `selected == None`.
- One item requires its sole MRU member to equal that visual item. Combined with the common condition, it is exactly that item selected first.
- Two items explicitly require distinct visual members. The only accepted MRU sequences are `[first, second]` and `[second, first]`; either sequence therefore also has two distinct members, no foreign member and no missing visual member. Selected still must be the first MRU entry. Duplicate visual items cannot accidentally pass because of the explicit inequality.

For three or more items the exact previous BTreeSet-based membership and uniqueness conditions remain, after the same common cardinality/selection checks. Missing MRU, length mismatches, absent selection, foreign items, duplicate MRU entries, visual duplicates and incorrect selected ordering retain their previous answers. There is no truncation, deduplication, repaired MRU or changed durable state.

The canonical input validator remains an actual consumer of the same predicate. Its InvalidSelection check still precedes the MRU predicate; duplicate application item ownership is still checked afterwards. Replacing this predicate's allocation strategy does not skip either canonical invariant or change which category is returned for malformed input.

The new `graph::tests::exact_tab_mru_requires_selected_first_unique_membership` covers valid empty/singleton/two-member orders, selected errors, size mismatch, foreign membership, visual/MRU duplicates, missing MRU and a longer-stack fallback example. It checks concrete expected truth values without adding a general oracle framework.

## Strict validation: success shortcut, unchanged invalid diagnostics

`validation.rs:610` returns early only inside `validate_tab_mru`, and only for `items.len() <= 2` when the complete exact-MRU predicate succeeds. That condition implies every existing MRU diagnostic condition is false: equal lengths, no duplicate MRU entry, no foreign or missing item, and selected equal to the most recent entry.

Every invalid short input falls through to the **unchanged original diagnostic body**. Missing MRU is diagnosed before the shortcut. Length mismatch, duplicate/foreign MRU diagnostics, missing visual members and selected-most-recent diagnostics retain their original loops, source order and multiplicity. Long stacks take that original body directly and retain its sets. This is not an early return from node or workspace validation.

`validate_tabs` still emits EmptyTabs/InvalidSelection before calling `validate_tab_mru`, and still runs the global item-owner/duplicate loop afterwards. Thus a valid local short MRU cannot hide an empty noncentral leaf, bad graph ownership, another pane's duplicate item, cycle/shared-node diagnostics or presentation errors. The existing invalid short-stack diagnostic-order regression is unchanged and still expects the full ordered vector. The new `strict_validation_checks_small_tab_stacks_completely` exercises both accepted orders and representative invalid small stacks through actual builder validation, including empty central state and duplicate/foreign members.

No changed acceptance or diagnostic behavior was identified. It is unnecessary to duplicate all old diagnostic code in a second test oracle because the invalid path itself is unchanged and the successful condition entails zero old MRU diagnostics.

## Boolean subtree query and callers

`workspace.rs:1012` adds `subtree_has_items`. The result is exactly `!collect_items_in_subtree(root).is_empty()` for the current query semantics: true iff at least one reachable existing Tabs node has a nonempty item list. The function copies no item IDs into an output inventory and returns on the first such leaf.

The traversal retains the collector's explicit heap stack, visited HashSet, reverse child push order and missing-node behavior. Each distinct reachable NodeId is processed at most once. A self-cycle, a larger cycle or shared child cannot make it loop; a missing start/child is ignored, as it was by the old collector. A cycle-only/empty reachable graph returns false; a reachable nonempty leaf returns true even if another malformed branch exists. The old collector also tolerated those branches and returned a nonempty vector. The query is not and does not claim to be a structural validator. Deep trees do not introduce recursion or stack-overflow risk.

Only the two intended emptiness callers change:

1. `ensure_nonempty_payload` returns the same EmptyPayload/Ok answer using the boolean query. Existing source/root/node/type proof checks still precede it; target semantics and actual policy remain untouched.
2. `cleanup_item_source_root` still requires the source root to exist and removes its root/presentation only when there is no declared central identity and no reachable item. It now checks `central.is_none()` first, avoiding an irrelevant traversal for a protected central root. The old collector was read-only and emitted no diagnostics, so short-circuiting that pure query changes neither workspace state nor error order. Last noncentral-item cleanup and empty-central preservation remain the same behaviors.

No partial item roster from this query becomes a policy or conservation fact. Full payload inventories are still collected where required; item-multiset ledgers/reconciliations and strict graph/canonical checks are unchanged. A malformed cycle or missing child is still rejected by the relevant owning validation boundary, not silently authorized by this boolean helper.

## Scope, tests and qualification boundary

The v1 candidate-bound Move owner, separate source/target full fingerprint comparisons, forged target-fact recapture, ordinary policy, batch staged-state handling and exact recovery authority are unchanged in this increment. The full transaction candidate, four item-multiset scans and one candidate clone are also unchanged. This is a bounded removal of unnecessary collection/allocation on success, not weaker validation or a new authority mechanism.

No new abstraction, compatibility alias, mirrored docking algorithm or broad proof script was added. Read-only `git diff --check` was clean for the four changed files. The source-level reasoning and the two focused new test tables are appropriate for the increment; final runtime test results and performance qualification remain the root's serialized work. In particular, v1 measurements must not be reused as proof that v2 reaches the 20% investment gate.

## Exact v1/v2 source identities

All seven v1 hashes below match the first measured receipt. Current hashes identify precisely this reviewed v2 snapshot.

| Source | Frozen v1 SHA-256 | Reviewed v2 SHA-256 |
| --- | --- | --- |
| `crates/dockspace/src/operation.rs` | `61f4ddde1d2bec8fae050cc455c1cf45161ad0949b56a74b85e95ebc117aeda9` | `dcb66eed0e2594e8ada5df5ff1890d558d2c5ad1a7d79315d4c2ee916357d41c` |
| `crates/dockspace/src/workspace.rs` | `941048a4a3f1905fa66b6f335fc04a6949afd3c8bffcd9c3e8da6526f3263128` | `dc64a2bcd6756f64145195c7c32ce560d9461b27aac849a4c48d673e59313e55` |
| `crates/dockspace/src/graph.rs` | `f2d48246a0cdb5d24a7a4ccb762ce92b286d2afa66b327c1d32a72b30dfa596f` | `8572b6a65a643dffd85d13ff876c41557be9c4697c543a0c22f79f4cf4b8f310` |
| `crates/dockspace/src/validation.rs` | `1e71cd27d8932e98e5ab56260d492242fa24f815cac13e2add73efdaaceb80bf` | `22398057fad003b31d8eaeb42d4459e299ce8541cc1f299d2d69875bd2412bff` |
| `crates/dockspace/src/canonical.rs` | `baf05788203163e013824c5ebf6de3ec9bae40b902e84d09d3c7f0e8c6a97c5c` | `baf05788203163e013824c5ebf6de3ec9bae40b902e84d09d3c7f0e8c6a97c5c` |
| `crates/dockspace/src/drop_resolver.rs` | `8b0af79ba94b039347db24cca17df6365b7793039c88559f61f046429b4b15e0` | `8b0af79ba94b039347db24cca17df6365b7793039c88559f61f046429b4b15e0` |
| `crates/dockspace/src/operation/tests/checked_move.rs` | `fe42f130f328253444a7d29bec738f58f0058a59f19afebd9be14195d04013dc` | `fe42f130f328253444a7d29bec738f58f0058a59f19afebd9be14195d04013dc` |

Final finding count: **0 blocking or required findings**. Freeze this v2 source after root verification, then measure it under a distinct executable/receipt and preserve both first-iteration failed-gate groups.
