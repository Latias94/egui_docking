# Frozen SecondaryMap slice: independent source review

Reviewed at 2026-11-06T17:37:59.671910+00:00. This verdict is bound to `arena-control/canonical.rs`, `arena-control/validation.rs`, `arena-control/runtime.patch` and `arena-control/source-receipt.json`, not the concurrently changing workspace. The `initial-sources` copies of both modules equal read-only `git show ea4161e:...`. No Cargo, build, test, timing, benchmark, repository edit, Git write or child agent was used.

**Verdict: no confirmed blocking correctness or scope issue in this frozen two-module slice. It is suitable for a reusable intermediate commit, subject to the already owned validation/evidence gates.** It is not qualification of the later sealed-owner or dense-layout WIP, and does not complete the new twofold goal. One material resource limitation must remain explicit: auxiliary storage is proportional to arena slot high-water/capacity, not just live node count.

## Frozen identity

| Input | SHA-256 |
| --- | --- |
| `canonical.rs` | `d2e8cd66e8fb47266a04e55249df4aea39cb54b2d9fe7306158881d7116c3a01` |
| `validation.rs` | `3fd3c88c262ad2824e5777e1706620a692d0af6d061439133854d1bfa4d105ea` |
| `runtime.patch` | `57d2393f479862ceb7768b47e8afe22d9fdcd3bff0216c2319ad47fa07fbbc7a` |
| `source-receipt.json` | `3c07aa50e076c913219420663007ebb5d6d1eef878efb32775a2571a5e0f3078` |

The two source hashes exactly match the corresponding entries in the frozen 259-input receipt. The receipt identifies base `ea4161e221ff8687f344b98f4223dec1efa38873`, binary `64fce95c07d3056710ed9846fb993621a16efb12e1a8cb165f00e949fcaf979d`, 2,024 ordinary core tests passed and three probes skipped. Those results belong to this frozen slice; this review does not rerun or extend their claim. The supplied two full comparison matrices remain evidence for root to retain, not performance data independently requalified by this source review.

## Live generation and insertion authority

The installed slotmap 1.1.1 `SecondaryMap::get`, `get_mut` and `contains_key` compare both slot bounds and complete key generation. `insert` itself is not a live-primary-map check and may silently refuse an older generation at an occupied secondary slot. The implementation does not rely on insert as validation:

- Validator `enter_node` (`validation.rs:524–565`) only inserts after `workspace.nodes.get(node_id)` succeeds. Missing root lookup still runs before walking; missing children append their existing error and are not inserted. A stale key that shares an index with a live key therefore cannot reuse the latter's visit record, replace it, or allocate up to an unvalidated arbitrary key index. `Exit` is pushed only after that successful insertion. The workspace remains immutably borrowed for the whole validation, so every Exit still has its same-generation Visit. Updating Done in place preserves the first owner.
- Canonical parent recording (`canonical.rs:495–504`) checks `workspace.nodes.contains_key(child)` before inserting. Parents are always the live keys obtained from primary-map iteration. Generation mismatch cannot become a false SharedNode or RootHasParent fact.
- The indegree table is initialized exclusively from current primary keys (`:574–588`); subsequent checked lookups reject absent generations. Input-node validation precedes this phase.
- Rewrites check the primary node on every Enter/Exit (`:280–308`). Rewrite performs in-place split replacement but neither removes slots nor allocates new node IDs. Cleanup/recycling happens only after all rewrite results have been consumed. The shared result table therefore never contains competing generations of one slot.
- Global reachable recording (`:640–655`) inserts only after a successful primary lookup. A contains check can bypass that lookup only for a key that this invocation already checked, with the primary arena unchanged during traversal. Cleanup retains by full key generation.

No bitmask, unchecked slot index, hash-only identity, implicit arena-membership assumption, or live-key validation removal is introduced. As before, generational keys do not encode the identity of their originating SlotMap; a key whose full bits equal an actual live workspace key is indistinguishable in the existing contract. This slice does not claim a new cross-arena provenance protocol.

## Traversal, owners and error order

The validator combines two maps whose entries were always created together. First Enter still records Visiting plus its original owner, traverses children in reversed-stack/left-to-right order, and completes on Exit. Re-entering Visiting emits Cycle; re-entering Done emits SharedNode with the retained first owner and current second owner. A repeated entry does not push a second Exit or overwrite owner. Root order, orphan discovery order, orphan-component start order, tabs/split checks, central checks and presentation checks are unchanged. Missing/stale children remain unmarked, so later references still yield their ordinary missing-child diagnostics.

Canonical input validation retains `validate_nodes → validate_acyclic → validate_roots`. The node/child iteration and all early return sites are unchanged. Kahn's zero-indegree queue now begins in slot order rather than HashMap order. This does not change a successful result or the reported cycle: validated children exist, indegrees cannot underflow on this graph, and after maximal zero-indegree elimination the residual positive-indegree set is queue-order independent. The final cycle witness still comes from the same `workspace.nodes.keys()` order (`:625–632`). Missing-child and internal-underflow branches remain checked; they are not newly reachable for structurally accepted input.

Before the shared rewrite table is created, validation rejects multiple parents, shared root nodes, root nodes with parents and cycles. Thus separate valid roots have disjoint live membership. The postorder schedule still writes every current subtree result before its parent reads it; `get(...).copied()` preserves the distinction between no result and the legitimate `Some(None)` for a pruned empty subtree. Keeping the root result instead of removing it cannot make another root use it. No new memoization shortcut skips a traversal or its errors. Roots remain processed in the same order, so the first EmptyRoot and protected-central behavior are unchanged.

The global reachable traversal uses the same root order and reversed child scheduling as the old `collect_reachable`. It moves set insertion after the successful lookup; on a missing child the function returns and drops the local table, so the failed-node insertion previously performed had no observable result. Item-multiset equality, final strict output validation and public clone-before-publish atomicity remain unchanged. Internal draft mutation on error still requires caller discard exactly as documented.

## Sparse capacity and multiple roots

Let C be the primary arena's slot high-water/capacity, N live nodes, E child edges and R roots. `SecondaryMap` allocates/initializes through the greatest inserted slot and iteration scans its slot backing; `with_capacity(workspace.nodes.capacity())` reserves C even if sparse. Parent, indegree, visit, rewritten and reachable tables therefore have O(C) worst-case auxiliary storage rather than the former O(N) hash-table storage. A heavily churned or deliberately over-reserved arena can materially increase temporary memory. The rewrite/reachable tables also live through later checks, so this is not a claim of reduced peak memory. No sparse-memory measurement is supplied here.

This is bounded by a real primary arena, not a forged child key: none of the new tables grows from an unvalidated missing index. Primary SlotMap iteration already scans slot storage, so whole-workspace passes already have a C term. The change does not make costs depend only on live N and should not be documented that way.

Importantly, `rewritten` is allocated once outside the root loop (`canonical.rs:225`), and `reachable` once for the entire root set (`:638`). A valid forest's subtrees are disjoint, so all-root rewrite and reachability walk aggregate N+E once; there is no new R×C table construction/reset. Existing per-root central reachability remains hash based and visits each disjoint root's subtree; this slice does not convert it into per-root arena-capacity scans. Validator visits are also shared across all roots/orphans. Thus sparse capacity is an explicit constant-memory/whole-arena tradeoff, not a newly confirmed superlinear multi-root correctness blocker.

## Recommendation

Commit only the exact frozen patch if root chooses the intermediate checkpoint; do not derive it from today's owner/layout WIP. Preserve the stated memory limitation and retain the successful/negative performance observations. There is no source-review reason to weaken diagnostics, introduce a second proof schema, add broad new scripts, or delay this intermediate slice for a larger rewrite. The later sealed-owner and dense-output changes require their own compilation and semantic review.
