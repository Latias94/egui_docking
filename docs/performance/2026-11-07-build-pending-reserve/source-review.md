# RootStructure pending-reserve design scope

Date: November 7, 2026. This is a read-only source-contract review of a proposed initialization change, not a candidate implementation, qualification receipt, or performance result. No build, test, formatting, benchmark, profile, candidate/control executable, or network request was run. Only this report was written.

There is **no source-contract blocker** to one narrowly scoped experiment: initialize the existing `Vec<BuildFrame>` in `RootStructure::build_rows` with `node_capacity_hint.clamp(1, 16)` and push its original first `Enter` frame. Preserve everything after initialization, the root lookup before initialization, and the existing hint producers. Keep ordinary Vec growth for all larger frontiers.

The actual parent remains undecided until Root finishes the separate equal-binary experiment. This report binds the graph sources to immutable `c7508eec3ac44138305797691244ae9c180d7680`, retained runtime `0e441c8`. It does not presume retention of equal-binary or the rejected source-nonempty change.

## Exact source boundary

Paths below are relative to `crates/dockspace/src/`. Each listed live file was independently compared byte-for-byte with its c7508ee Git blob and matches:

| File | SHA-256 |
|---|---|
| graph/root_structure.rs | `a89bf210b4836d10380007e1d5594ad6c76281a338b12d10f0408ba7ee98eca7` |
| graph/authority.rs | `89d0d337cdc2365caf62f5dd2d48d6f9132b21c2d4b882e9c24096cc6602a600` |
| canonical.rs | `b2f1ab4bd5b7e5933966c54792b30f4d257f823f172a75c8686f9260af0ea517` |

The only proposed edit is root_structure.rs:126–130, the existing one-entry `pending` initialization. Request capacity using the existing scalar, then push the same `BuildFrame::Enter { node: record.node, parent: None, child_slot: None }`. Keep `Self::default()` and the `MissingRoot` lookup at lines 121–125 in their original positions. No root-kind lookup, helper, new field, alternate builder, inline container, scratch cache, or additional capacity policy is necessary.

Do not change the `nodes`, `children`, `postorder`, `ordered_rows`, or `split_rows` reservations at lines 146–159. In particular, do not move the node lookup or first-row reserve block earlier to avoid overreserving a Tabs root. That would expand this experiment's scope and alter its allocation/lookup sequence.

The existing hint flow is:

| Path or state | Existing behavior that must remain |
|---|---|
| Full build, issuance, import, general completion, raw/full canonical completion | `RootStructure::build` supplies zero at root_structure.rs:95; clamp therefore requests one pending entry, as before. Full placement installation remains in the original callback. |
| Checked Item completion of an Editing root | `build_after_item_move`, lines 101–112, uses zero for unknown previous count, otherwise `previous_nodes.saturating_add(2)`. Only this already-hinted path can request more than one initial pending entry. Its placement callback remains empty because the checked writer already repaired actual placement. |
| Repeated `begin_root_edit` | authority.rs:110–116 captures the Ready body's node count once. Repeated calls on Editing do not replace or inflate it. |
| Raw take/restore | authority.rs:120–139 preserves the existing take/restore protocol, including an unknown count of zero while structures are taken. No old body is retained merely to size pending. |
| Already Ready Item completion or missing owner | authority.rs:165–172 preserves the early/no-build behavior. A semantic-only center move retaining its Ready body does not acquire a new pending allocation. |
| New root or unknown previous body | Zero remains “no count available,” not an authority claim that the tree is empty. Its initial pending capacity remains one and can grow normally. |

The hint remains an allocation hint. It does not determine node membership, stop traversal, authorize placement, or change the checked writer's validation obligations.

## Traversal, errors, and ownership

The frame ordering at root_structure.rs:131–200 must remain unchanged. Each split appends its `Exit` frame and then reversed child `Enter` frames; the LIFO traversal consequently visits children in their original order and records parent postorder after descendants. Retain the original child-slot writes, payload `Arc::clone`, `on_node` call, postorder pushes, directory construction, and work-counter completion event.

Increasing capacity changes neither Vec length nor initialized entries. Spare capacity contains no additional live BuildFrames, graph values, or roots. The first `push`, all later `push`/`extend` calls, and the same pop loop still define the entire execution. No public reference, node generation, root identity, parent index, item/MRU history, or owned projection value changes.

The original `MissingRoot` remains before pending allocation. The original `MissingNode` lookup remains before row creation, placement callback, and child processing. Split assertions and all later fallible operations stay in their original order. As with any infallible reserve-size change, allocation failure/resource-exhaustion timing can differ; the proposal does not add a recoverable allocation error or promise identical OOM behavior.

The stack remains an ordinary growable heap Vec. Sixteen is an initial-capacity cap, not a maximum length, depth, arity, or traversal-work bound. A split with sixteen children already needs its Exit plus sixteen child frames after expansion. Deep trees retain ancestor Exit frames and unvisited siblings. Both cases can exceed the initial capacity and must use the same standard growth path. There is no recursion added and no fixed-size failure branch.

A balanced sixteen-leaf binary tree can need nine pending frames because each ancestor retains an Exit and an unvisited sibling. This explains why this is a different storage need from a projection continuation stack with a smaller initial reserve. It does not prove that sixteen covers every edited sixteen-pane shape or that one capacity is optimal.

Pending is local temporary storage. Its destruction and all error unwinding remain private to the builder. No RootStructure Arc is cloned for the hint, no additional Arc owner persists, and the change adds no Arc::make_mut or old-body mutation. Candidate isolation, rollback, unfinished-root rejection, and previously owned output follow their existing paths.

## Costs and unresolved benefit

The prior diagnostic-location review identifies the saved `complete_item_move_structure +1352` reserve as this pending Vec's child extension. I read that allowed report for attribution; I did not repeat disassembly or sampling. Its inclusive sample counts are not allocation counts, clock shares, or nanoseconds saved.

The candidate can avoid some early Vec growth and copies in hinted Item rebuilds. It still allocates the initial pending buffer and still traverses the whole rebuilt root. All row buffers, immutable split ownership, directory sorting, Arc construction, actual placement repair, and complete output remain. Runtime complexity is unchanged.

The allocation cost is explicit. The reviewed diagnostic executable has a 40-byte BuildFrame, so requesting one versus sixteen entries changes the requested element storage from 40 to 640 bytes on that executable; this is not a portable Rust layout guarantee or an exact allocator-footprint promise. Unused capacity does not initialize sixteen frame values.

An edited root that ends as Tabs can request up to sixteen pending entries from its old-node hint even though it only uses one. The existing row buffers still inspect the actual Tabs kind and request their original small capacities. Hinted shallow roots can also overreserve, while zero-hint deep or n-ary roots get no larger initial reservation. The edit therefore cannot be described as eliminating all builder growth, all allocations, or all small-shape overhead.

The ordinary logical-work CSV should remain byte-identical: no row, payload ownership event, root completion, canonical pass, reference check, or output element was removed or added. Those counters do not count pending capacity or allocator bytes. A changed logical ledger would require investigation rather than being accepted as the intended benefit.

No source argument establishes a latency win. In particular, unaffected center and resize workloads still need the normal complete comparisons; they cannot be exempted because their source path is unchanged.

## Existing coverage and its limits

The following are existing registered tests read from the exact c7508ee sources. They were not executed by this review. Integration paths beginning `tests/` are relative to `crates/dockspace/`; internal test-module paths are relative to `crates/dockspace/src/`.

| Existing witness | Relevant contract and limit |
|---|---|
| tests/transaction_atomicity.rs:1069, `deep_root_fingerprints_and_descendant_checks_are_iterative` | Builds an alternating-axis binary chain of depth 10,000 with side leaves, retaining a genuinely deep valid root. Builder issuance reaches the full zero-hint RootStructure build. It then checks descendant rejection and unchanged workspace. This is real growable/deep full-build coverage, not direct coverage of a nonzero-hint Item rebuild. |
| tests/canonical_graph.rs:334, `deeply_nested_singletons_use_bounded_heap_traversal` | Canonicalizes 10,000 singleton wrappers, verifies their removal and preserved empty central identity. The final builder root is already a single Tabs node; do not mislabel this as a 10k RootStructure pending-stack test. |
| tests/canonical_graph.rs:100 and :150, empty-leaf/wrapper and promoted-central tests | Verify empty noncentral removal, exact empty central retention, promotion, item ownership, and a valid three-child canonical split. These cover empty and small n-ary semantics, not a greater-than-sixteen hinted frontier. |
| operation/tests/owner_reference.rs:813, `initial_issuance_preserves_empty_workspace_and_builder_presentation_errors` | Checks zero-root issuance, no phantom references or surfaces, MissingRoot, and exact presentation-validation errors. With no roots, there is no build_rows call or pending allocation. |
| operation/tests/local_item_move.rs:113, `local_item_moves_match_complete_normalization_and_exact_mru` | Exercises both axes, last/nonlast source, central variants, selected/history states, destinations and target kinds. Compares complete candidates/reports against the general path, checks exact MRU/no-op references, validates ownership, and invokes assert_actual_placement. This directly covers hinted Item rebuild semantics. |
| operation/tests/local_item_move.rs:174, `last_item_retires_only_its_source_presentation_and_candidate_can_be_discarded` | Checks main and contained last-item retirement, unchanged original, candidate discard, exact target MRU, and deterministic retry/report. |
| operation/tests/local_item_move.rs:335, plus the existing input-weight and source-collapse witnesses at :387/:451 | Preserve general-path rejection equality and rollback on failed local Item normalization. No new allocation-dependent shortcut should change these errors. |
| operation/tests/owner_reference.rs:282 and :884 | Cover staged transient structure, final normalization, late-error rollback, retired placement cleanup, and rejection of an unfinished body before querying leaf constraints. A completed owned projection remains valid after both workspaces are dropped. |
| graph/copy_tests.rs:360 and :395 | Check late batch failure with unchanged original payload addresses/references, and generational slot reuse with independent candidate issuance. |

The shared assert_actual_placement helper at owner_reference.rs:33 onward also checks exact row membership/cardinality, postorder cardinality, row lookup, direct-child joins, and shared split payload identity against actual storage. Keep these assertions unchanged.

I did not find a dedicated existing witness for a greater-than-sixteen pending frontier reached specifically through a nonzero Item hint. That is a coverage limit, not a semantic reason to block an initializer-only change: the same standard Vec growth operations remain in place, and the 10k full-build witness already exercises the shared growable traversal. A new test that merely asserts capacity or reproduces Vec behavior would mirror the implementation. Root should use the existing ordinary, deep, local/general-oracle, rollback, work, and complete-vector gates; any actual broader source change requires a separate review.

| Exact reviewed test file | SHA-256 |
|---|---|
| operation/tests/local_item_move.rs | `68020548cbdbd968b09cbfdcb0dbb5a43217074cd0c504f528546d2a243815d2` |
| operation/tests/owner_reference.rs | `0d6a8d0f5931976a1561790a48d53273c8f68e5983d6ad25c8ff7989d4aa7199` |
| graph/copy_tests.rs | `fa5478d74b3e851fa74f26dfeb6d0f1bd2176b091ceca0bd3048697cc1be39c4` |
| tests/canonical_graph.rs | `c3f93af0b3e30173b24a281011c05ee14120e2415e15d56ce720b575b1bca423` |
| tests/transaction_atomicity.rs | `b0905a7b19777a3ce05b09edc8c3e05105cecccb46c774a76b4c23324a11d19e` |

These five test files also match c7508ee byte-for-byte. Once the equal-binary decision establishes the actual retained parent, bind the eventual candidate to that parent's full source pair and one initializer hunk. This report authorizes no implementation or retention and claims no measured benefit.
