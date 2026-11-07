# Borrowed topology implementation review

**No source-level blocker.** The implementation is the proposed private borrowing change, with the two durable output owners preserved. It is suitable for the declared early qualification and complete-operation screen. This review does not establish compilation, test success, code generation, or performance.

## Exact reviewed sources

Package: `/tmp/dockspace-imgui-twofold-20261107-ea4161e/post-main-continuation/borrowed-topology-source`.

| Artifact | SHA-256 |
| --- | --- |
| `base/crates/dockspace/src/layout.rs` | `01f21abad6c4d71856dbf17ae9eec90a97d7fbe7717bb2e6da87895f2fed5dcd` |
| `candidate/crates/dockspace/src/layout.rs` | `ff24472a4680c5294d5d324630ddf83ed018d6ad1e7fbdf9868e1bee9ad9de95` |
| `runtime.patch` | `b91a19ceab5a81d57c4d8391656134233bac118dd529b9abe36c15c7909b33b0` |
| `predeclared.json` | `42013518c51c8136363b1168aa0d17aadf6efbbe7451a25ae849725dff6c0ad2` |

I compared the complete base/candidate files. Their actual differences match the five displayed patch hunks: direct topology acquisition, delayed call argument, private helper signature, output-clone/traversal arguments, and raw call argument. The live `layout.rs` matched the candidate during this review. No test, solver, measurement, traversal, validation, timer, exporter, or public output type is changed.

## Ownership and error paths

The direct entry at candidate `layout.rs:504–506` borrows the Arc from the already borrowed workspace. That workspace cannot be mutably edited through this call, and its owner keeps the immutable structure alive throughout leaf measurement and projection. Missing-root, override, and unfinished-root checks retain their original positions.

`project_measurements` accepts `&Arc<RootStructure>` and still clones it separately into `NodeMap` and `SplitMap` at lines 659–660. Both map types store owned Arcs and owned value buffers; the returned `LayoutProjection` has no input lifetime. Independent map/result clones and outputs that outlive the workspace therefore retain the same ownership mechanism.

Errors before map construction drop the owned measurement values normally. Errors from `project_subtree` drop the partial local projection, including both acquired output Arcs and their buffers, without returning it. The source Arc remains owned by the workspace, delayed measurement, or raw temporary. No publication, rollback, or failure-recovery boundary is added.

The delayed `RootMeasurement` still stores its own `Arc<RootStructure>` and `Vec<NodeResult>` (`layout.rs:541–549`); its producer still acquires that Arc at lines 814–818. The consuming call borrows `self.topology` while moving the disjoint `self.values` field. The struct has no custom `Drop` implementation. Its remaining topology owner stays alive through the call and is released afterward. Bounds-dependent maximum completion, override validation, and independently held minimum measurements remain unchanged.

For `project_draft`, `&Arc::new(topology)` creates an owned temporary that lives through the complete call expression. The helper acquires both output owners before returning success; the temporary then drops. On failure, partial output owners drop inside the helper and the temporary drops after the call. Raw checked compilation, central membership collection, shared-subtree revisits, and last-write behavior are unchanged.

The source reduction applies to the direct synchronous entry's extra temporary retain/release. Delayed and raw paths keep their existing input owner; this patch does not claim to remove their necessary owner or any topology allocation.

## Numerical, callback and output contract

The full-file diff leaves all arithmetic, error-producing expressions, callback invocations, override checks, minimum/maximum composition, raw bool propagation, central routing, release prefix checks, and continuation ordering unchanged. Both output Arc clones remain in their original construction order. The result buffers and every fresh field remain materialized in the same code.

The borrow has no mutable path into the topology and does not escape through the output. There is no new cache, proof, flag, provider, executor, or API lifetime for consumers to manage. Code generation may still change because the private argument representation changes; source equivalence supplies no gain estimate.

Existing semantic coverage is appropriate:

- `dense_projection_keeps_identity_order_and_owned_old_outputs` drops the workspace and original result before checking retained/cloned output.
- `owned_node_rows_keep_previous_outputs_after_late_geometry_error` covers a later failed projection while earlier outputs remain readable.
- `central_ancestor_depths_keep_fresh_reader_order_and_delayed_minimum_projection` and `measured_minimums_preserve_projection_constraints_and_resize_overrides` cover fresh callback order and delayed maximum/override behavior.
- `raw_shared_central_subtree_keeps_membership_in_both_parent_branches` exercises the raw adapter and shared occurrences.

I found no missing semantic case requiring a new test specifically for this private borrow. A test counting Arc clones would mirror the implementation. The existing ordinary gates and full work/output comparison remain necessary; none was run by this reviewer.

The predeclared complete/small-layout thresholds, additive confirmation, medium/large target protection, and repeated material regression rule are unchanged. This review approves the implementation's bounded scope for evaluation, not retention or a final stopping decision.
