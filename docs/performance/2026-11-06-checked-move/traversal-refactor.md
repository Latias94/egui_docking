# Canonicalization and validation traversal refactor

Date: 2026-11-06. Base/lane contract: `a2bcabc1440caa664fb5f4a64c4669c4eecfdce4`, `lanes.json` in this directory. Read `rust-best-practices/SKILL.md` before modifying Rust. Only `crates/dockspace/src/canonical.rs`, `crates/dockspace/src/validation.rs`, and this receipt were written. No Cargo, C++ compilation, test, benchmark, Git write, other-source edit, or nested agent was performed.

## Implementation

`canonical.rs:266`, `rewrite_subtree`, now borrows the arena node on both Enter and Exit instead of calling `Node::clone`. Children are scheduled with reversed copied IDs, retaining the original iterative depth-first/postorder execution order. Empty non-central leaves and protected central identities retain the same result semantics.

`canonical.rs:313`, `rewrite_split`, now reads its split's children/weights directly from the live arena. Its initial child-result scan still verifies every required child result in the existing order. It identifies pruned/promoted children and same-axis flattening before allocating replacement storage:

- Zero surviving children return no node; a singleton promotes the sole surviving child.
- If at least two children survive unchanged and none require flattening, return the original split immediately. Existing child/weight vectors and exact weight bits remain untouched; no temporary rewritten/flattened vectors are allocated for this case.
- A changed split allocates a flattened row buffer, copies only the IDs/weight scalar values needed for that rewrite, and borrows same-axis grandchildren/weights rather than cloning their vectors.
- The original left-to-right effective-weight multiplication and `SplitWeight::normalize` remain the replacement rule. The arena slot is assigned only after replacement children and normalized weights have been constructed successfully.

The private helper re-fetches its split from the arena because it owns the later mutable arena access. Its `TraversalInvariant { stage: "split rewriting" }` branch guards a wrong-kind/missing internal invocation. The only caller has just matched that same live node as `Node::Split` under exclusive workspace access, with no intervening mutation; no validated input gains a new public failure path. This is a local helper check, not a proof cache or validation framework.

`validation.rs:509`, `Validator::enter_node`, copies the existing shared workspace reference into a local binding and borrows its Node. This separates the immutable graph borrow from mutable validator bookkeeping/errors. Tabs and split slices are passed directly to existing validation methods. Only Copy fields (axis, selection, child IDs) are copied. The full Node/items/children/weights clone is removed.

## Preserved boundaries

No validation pass was removed or merged. Canonical input validation, acyclic/root checks, postorder rewrite, protected central handling, reachability sweep, before/after item multiset conservation, and final strict workspace validation remain in the same order. The public atomic wrapper still clones its complete candidate and publishes only after success; the internal discardable-draft boundary is unchanged.

Validator mark/owner updates, Exit scheduling, cycle/shared-node checks, same-axis reporting, per-split validation, reverse child scheduling, per-tabs MRU checks, item-duplicate accounting, orphan traversal, central validation, and presentation validation retain their order. No recursion or per-node retained references were added to either traversal stack; stacks continue to contain node IDs and small control values.

For changed splits, the initial results scan runs before reconstruction. The second lookup during reconstruction reads the same unchanged `results` table; missing child results have already returned the same structured error. Reconstruction borrows children without mutating them and builds owned output before the final assignment. Same-axis descendants remain in the arena until the existing later reachability sweep, so the borrowed flatten inputs are live throughout the local operation.

## Added regression and existing coverage

Added `validation::tests::validation_preserves_preorder_and_tab_diagnostic_order` (`validation.rs:915`). One malformed graph combines same-axis nesting, non-normalized nested weights, invalid selection, malformed duplicate/foreign MRU entries, duplicate item ownership, and a later shared-node edge. It asserts the complete eleven-error vector in exact discovery order, covering ordering across node-level, tabs/MRU, descendant, and repeated-edge diagnostics. The expected sequence is checked directly; no clone-count instrumentation or implementation-mirror test was added.

Existing tests already cover the affected canonical semantics and should be reused by root:

- `canonical_graph::same_axis_splits_flatten_with_composed_weights_and_reach_a_fixed_point`.
- `canonical_graph::empty_tabs_singletons_and_same_axis_wrappers_are_removed_without_rebinding_central`.
- `canonical_graph::promoted_root_keeps_the_exact_empty_central_leaf_identity`.
- `canonical_graph::empty_orphan_nodes_are_swept_but_item_bearing_orphans_fail_closed`.
- `canonical_graph::invalid_graphs_return_structured_errors_without_partial_rewrites`.
- `canonical_graph::deeply_nested_singletons_use_bounded_heap_traversal` (10,000 levels).
- Existing canonical unit tests for late failures, exact error reporting, candidate clone accounting, and rollback.
- Existing validator unit tests for cycle/shared/orphan behavior and non-finite weights.

Root's checked-Move and transaction regressions remain necessary integration qualification; this lane did not alter operation/workspace/transaction sources.

## Validation performed and remaining source risks

Successfully ran direct formatting:

```text
rustfmt --edition 2024 crates/dockspace/src/canonical.rs crates/dockspace/src/validation.rs
```

Reviewed the exact two-file diff and the existing regression coverage. Production `.cloned()` / vector `.clone()` calls in the targeted canonical/validator traversal paths are gone. Remaining `.clone()` calls in `canonical.rs` belong to existing tests and the existing atomic candidate helper boundary is untouched.

Compilation and runtime verification were intentionally not run. Root must qualify the NLL borrow scopes and the new exact diagnostic test with the serialized Rust build/tests. Source inspection indicates the immutable arena borrows end before replacement assignment and the validator's local shared reference is independent of mutable bookkeeping; rustfmt confirms parsing, not borrow checking.

The conditional rewrite scan adds a lightweight preflight over split children and avoids allocations for unchanged splits. Changed splits still allocate their actual replacement output and perform normalization. Global validation/item-conservation passes, their HashMap/HashSet/BTreeMap allocation costs, and the complete transaction candidate clone remain. No timing improvement is claimed before root's measurements, and this change does not weaken their contracts to obtain a smaller operation count.
