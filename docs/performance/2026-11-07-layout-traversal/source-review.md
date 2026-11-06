# Independent review: one layout traversal-state owner

Reviewed on 2026-11-06. Scope: the current changes to `crates/dockspace/src/layout/topology.rs`, `crates/dockspace/src/layout/topology_tests.rs`, and `docs/plans/2026-11-06-003-layout-traversal-state.md`. The frozen traversal reference and existing topology tests were read as supporting evidence.

**Verdict: no confirmed correctness or scope issue requires a change before formatting and freezing the candidate for verification.** The implementation follows the bounded plan. This is a source review, not a test or performance receipt; no Cargo, test, executable, timing, profiler, Git write, repository edit, or child agent was used.

## State ownership and indexing

`topology.rs:60–83` replaces `completed` plus `visiting` with one command-local traversal map and the existing measurement vector. The correspondence is exact:

| Prior representation | Current representation | Meaning on repeated Enter |
| --- | --- | --- |
| Neither collection contains the node | No entry in `rows` | Perform the first lookup and allocate its dense row. |
| `visiting` contains the node | Row exists and `values[row]` is `None` | Return `Cycle { node }`. |
| `completed` contains the node | Row exists and `values[row]` is `Some` | Reuse the completed row. |

The new unchecked vector indexing at `topology.rs:69` is supported by a local invariant: a row is registered only after both `topology.nodes` and `values` receive their corresponding entries. Their lengths advance together, entries are never removed or reordered, and `rows` is not exposed outside compilation. A row cannot refer beyond `values`.

A leaf becomes complete only after `leaves.get(node)` succeeds (`:85–90`). A split becomes complete only after all required child measurements and `compose_constraints` succeed (`:119–147`). Execution is synchronous against an immutable workspace; no Enter can interleave between assigning a completed measurement and updating its split kind/postorder. There is no additional authority object, persistent cache, version-based trust, or duplicated completion state.

## DAG reuse and diagnostic order

Completed shared subtrees still bypass traversal, while an edge to an active ancestor still returns Cycle. Reversed child scheduling and placement of Exit frames are unchanged (`topology.rs:102–103`), so the traversal retains its original left-to-right depth-first order.

The important error paths remain ordered as before:

- A repeated active node is rejected before another workspace lookup (`:67–76`).
- A first missing node fails at the same workspace lookup. Moving its transient registration relative to the former `visiting.insert` has no observable effect because compilation returns immediately and drops all local state.
- Split cardinality is checked before scheduling children (`:95–103`).
- Missing leaf constraints and composition failures stop traversal at the same point, before later sibling faults.
- Exit now looks up child rows in the broader `rows` map, but it still requires `values[child_row]` to be `Some` (`:119–125`). Existence of an unfinished row is not accepted as a completed child. Absent or incomplete required children retain the same `MissingNode { node: child }` result.
- Final completeness conversion and central reachability validation remain in place (`:151–156`).

The same three-state interpretation is visible in the frozen reference's measurement map plus active set (`layout/reference.rs:129–195`). The refactor changes their storage rather than traversal policy. Acceptance of a completed shared subtree in the direct layout path is existing behavior; it does not make a shared DAG a valid strict workspace forest.

## Tests and scope

The new `completed_subtree_reuse_keeps_active_cycle_and_leaf_error_order` test (`topology_tests.rs:409`) is relevant and bounded. It first builds a valid alternating-axis tree, then deliberately introduces a completed shared subtree and optionally a back-edge. Its four combinations compare the complete result with the frozen reference, with separate expected-error assertions:

- Complete constraints and no back-edge succeed with shared-subtree reuse.
- Complete constraints and a back-edge return `Cycle { node: root }` after the completed subtree is reused.
- Missing first-leaf constraints win over the later back-edge, with or without that back-edge.

The existing randomized full-projection comparison (`topology_tests.rs:56`), cold/warm minimum-measurement checks, same-version structural-edit checks, and malformed-topology error-priority tests remain unchanged. These provide the intended broader verification surface; this review does not claim they have been executed on the candidate. No extra protocol framework or large fixture duplication was added.

The runtime diff is confined to compiler-local traversal bookkeeping and removal of the `HashSet` import. `matches`, warm `measure`, topology ownership, solver behavior, output construction, transaction validation and benchmark boundaries are unchanged. The plan accurately limits the slice to this implementation detail, retains exact structural reuse checks, and calls for independent semantic and formal timing verification. No rollback or batch contract is weakened.

## Cost and evidence wording

The change removes one hash collection, active-path insert/remove operations, and one hash-state creation per compilation. The old active set's high-water size follows DFS depth on the balanced fixture; it was not a second full N-node completed table. The new map also inserts on first entry rather than completion, so map growth and hash insertion order change. A future timing difference cannot be attributed solely to removal of one allocation.

This is a new bounded layout simplification. It does not establish the cause of the historical resize shifts. Root's completed setup control and history reruns should remain separately identified evidence, including results that do not reproduce the earlier direction. The existing plan correctly states that unchanged source and setup controls cannot prove a unique cause. There is no source-level reason from this review to delay the planned formatting, semantic checks, or frozen comparison.
