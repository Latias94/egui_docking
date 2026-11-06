**Findings first**

No source correctness or integration blocker was found in the current exact patch `3cb323c6d799ce448e431c1944d040ada21c7bc4a55fba215cfcd3534eebbfee`. It is suitable as the next independent storage slice, followed by Root's serial compilation, existing behavior qualification, and separate measurement. No source correction or additional test framework is requested.

The patch retains all `3E` child-result f64 values, complete immutable outputs, the existing solver and traversal, and the public three-slice contract. One material limitation is intentional and documented: one contiguous `3E` allocation has a different resource-failure boundary from three separate E allocations. This review does not assert identical maximum capacity, OOM behavior, allocator activity, timing, or twofold-goal progress.

Only `/tmp/child-extents-slab-current/review-final.md` was written. The previous `review.md` was read as background and preserved. I independently examined the current complete two-file base/tree, exact patch and receipts, frozen dependent source and distinguishing tests. No Cargo, compilation, tests, benchmark, timing, formatting, repository/candidate/fork/cache mutation, or sub-agent execution was performed.

**Exact candidate identity**

| Source under `crates/dockspace/src/` | Base SHA-256 | Candidate SHA-256 |
| --- | --- | --- |
| `layout.rs` | `325f56f16cacbde46cce531461c8830417a6aa8ff4af06f6c87145a7528963df` | `e4597343ab8b0f8d69f94bebf7e23e190905dace9e77b942b2187c2ef555d26c` |
| `layout/projection.rs` | `d86c008dd78c5f1ed805de76b71d102a29dd3dab4e6ee37ca8f3cb74839edfb0` | `d770e03663941e840e5e37222d9663092c3e699f27b51dee535d301abc492c60` |

The patch is exactly two runtime paths, 4,450 bytes, +33/-16. I reconstructed both patch results in memory from their saved base, checking every hunk's context; both equal the complete candidate tree. All per-file bytes/hashes, exact path sets, sorted set hashes, manifest/README/READY bindings, and the eight claimed unchanged source regions matched independently.

| Artifact | SHA-256 |
| --- | --- |
| Current patch | `3cb323c6d799ce448e431c1944d040ada21c7bc4a55fba215cfcd3534eebbfee` |
| Two-path base set | `a55a00fcc4d62b6fafa5c294780a629138a57d365cb72f578127b62dc3b42fc2` |
| Two-path candidate set | `862570a7bf610c50607d9e42ddedbab83203a2ba051beadb150131e2c7578647` |
| `base-manifest.json` | `7b106489bca5577156ddce9c00ebb9a9a5ec1f7cf1e4f825a1341526c1e0cefc` |
| `manifest.json` | `66cef3bd21d57f6b6fdfd399c8fef7259fc865a5d3e985ab50952cfbd63a004a` |
| `scope-checks.json` | `5d3ace80ebc68ded74dcf76809b1ae66b03d861528d18e830db242f06dffb4ce` |
| `README.md` | `0b9868fd5806bb845146ac0ea620d36275b222ecf8ff1693c13357b3d1d34caf` |
| `READY.json` | `dfd42eff8514241063e2a9a914fd8d413ba1be01f15ffc14ffdc3c51a831ca15` |
| Previous review, used only as background | `ea454ef0fc8b703bc4405a0e663502cf50c968c2b5b13837fa846f8f9b010830` |

Both saved input files equal the frozen single-NodeResult outputs named by `base-manifest.json`. Both current repository files also matched those saved inputs at this review's readback. The slab therefore currently needs no source rebase in its two target files. That observation does not authorize overwriting newer files if they subsequently change; apply the exact two-file delta and retain later counter/owner/native work elsewhere.

**Complete storage and safe partitioning**

`tree/.../layout/projection.rs:203–228` derives E from the same immutable `topology.children.len()` and allocates a fresh zero-initialized `Vec<f64>` of length `3E`. The two safe splits produce exactly:

- minima: `[0, E)`;
- maxima: `[E, 2E)`;
- solved actual extents: `[2E, 3E)`.

No extent scalar is omitted or computed lazily. The initial values remain positive `0.0`. The private slab has no resizing/replacement mutator in this candidate, and the topology remains owned through Arc; construction and derived Clone preserve their length relationship. The accessors use `split_at`/`split_at_mut`, so the mutable slices are disjoint and require no unsafe pointer arithmetic or aliasing claim.

`SplitMap::at_row` (`projection.rs:285–307`) applies the original per-split child range to each segment. RootStructure's flat edge indexing and each range's order/length are unchanged. The same logic therefore applies to binary, n-ary, nested, and diagnostic raw-DAG layouts. A leaf-only E=0 root has an empty child slab, with no child-buffer allocation; this says nothing about the other allocations in a complete projection. Empty splits/results retain their original map behavior.

Payload storage remains **3E f64 = 24E bytes**. For the balanced binary fixture, total E is `2 * (panes - roots)`. The README's 720/576-byte child payload inventories for 16 panes at one/four roots, and 49,104/48,960 bytes for 1024 panes, are arithmetically correct. Source-level child-buffer requests change three to one per root with E>0, including when cloning; one/four nonempty roots remove two/eight such explicit backing requests. These are source inventories, not measured allocator calls or total-memory savings.

**Capacity and error contract**

`checked_mul(3)` at `projection.rs:204–208` prevents unsigned length wraparound in all build profiles. Its failure is an internal capacity panic with the explicit message, not a new `LayoutError`. `vec![0.0; child_storage_len]` still applies Vec's byte-layout/capacity checks and ordinary allocation behavior.

The guard does **not** prove that the new allocation can represent every state that the old three allocations could represent. Considering only f64 buffer capacity, the old individual buffers each require `8E <= isize::MAX`; the new contiguous buffer requires `24E <= isize::MAX`, in addition to the checked element-count multiplication and the platform's other allocation constraints. Fragmentation, size classes, and available contiguous memory can also distinguish the cases below those theoretical bounds. The existing topology and other projection buffers impose their own size limits; these formulas are not reachable-workspace promises.

The constructor now checks length multiplication before allocating its outcomes/splitter/slab fields. Resource-panic/OOM ordering need not equal the original sequence of allocations. The prior implementation likewise did not return a recoverable layout error for capacity exhaustion. The public `solve_axis` remains byte-identical and retains its validation-before-output-allocation behavior. For successfully constructed projection storage, all numerical/override/geometry diagnostic steps retain their existing order. The README states the capacity limitation accurately; no oversized synthetic test or new error/proof layer is necessary for this bounded change.

**Solver writes, borrowing, and traversal**

Only three storage substitutions occur in `layout.rs`:

1. At `:1002–1010`, borrow the three slab regions and write the same minimum/maximum child range in the original zipped constraint order.
2. At `:1032–1039`, pass the original range of the actual-extents segment into the same `solve_axis_into` call with unchanged extent, weights, constraints, central index, and scratch eligibility.
3. At `:1123`, read the same flat actual child element when forming the next child rectangle.

Minimum/maximum slices have no further use after their write loop; the actual slice's final use is the solver call. Those borrows end before `set_outcome` reborrows SplitMap at `:1040–1042`. No slice, input reference, or scratch buffer is retained by the returned owned result. The outcome fields and splitter storage are independent of the slab.

All eight recorded unchanged regions matched their base bytes, including the full public solver core, fresh measurement/override and NodeResult construction, splitter geometry math, and the complete `validate_inputs`-to-EOF numerical suffix. `validate_inputs` still returns `Result<(), LayoutError>`; the weight total, desired-size calculations, both assigned-size sums, grow/shrink logic, minima/maxima branches, and final outcomes remain original. The rejected solver aggregate and weight fusion are absent.

The continuation state still carries `pending_child_size: Option<f64>` (`layout.rs:898–909`). A child's subtree finishes before its following gap and next child rectangle are constructed (`:1082–1139`). No frame compaction or pending-size reread transformation is included. Reading an actual extent from the segment does not change any arithmetic or rounding sequence. A numerical failure still abandons only the private projection being constructed.

**Immutable behavior and old outputs**

`SplitMap` derives Clone (`projection.rs:193`) over an owned Vec. It deep-copies all 3E scalar results while sharing only the immutable topology. `LayoutProjection` still owns/clones both the single `NodeMap<NodeResult>` and SplitMap (`:373–380`). Later projection construction allocates fresh result storage (`layout.rs:632–645`); it does not mutate a prior slab or reuse caller scratch as output ownership.

Public `SplitProjection` fields remain the original three borrowed slices, with the same names, values, lifetimes, child order, and cardinalities (`projection.rs:162–183`). `get`, `contains_key`, `len`, keys/values, ascending identity iteration, reverse iteration, ExactSizeIterator and FusedIterator implementations are unchanged. Full generational identity lookup remains at the topology join. PartialEq still compares complete identity-ordered borrowed projection values; no pointer equality, truncated result equality, or new Eq implementation is introduced for floating-point outputs. Debug still renders the same value map through its unchanged custom formatter (`:311–320`).

The result can survive dropping its input measurements, constraints, original Workspace, or another projection. A cloned result owns separate scalar backing storage. A failed later layout cannot corrupt a retained old result, because neither its slab nor its other result buffers are shared mutably.

**Existing distinguishing coverage and limits**

No test is changed by the patch. I read the retained tests and the independent oracle from the frozen dependency paths, after verifying their source hashes:

| Existing source coverage | What distinguishes a wrong slab |
| --- | --- |
| `layout/topology_tests.rs:90`, `dense_layout_matches_frozen_traversal` | Complete value comparison over nonuniform nested/n-ary constraints, central cases, bounds/metrics, and both direct-constraint and minimum-only routes |
| `:126`, `dense_projection_keeps_identity_order_and_owned_old_outputs` | Full result capture, changed later layout, clone retention after dropping Workspace/original output, identity/preorder/value iteration |
| `:211`, `sparse_arena_projection_rejects_retired_generations` | E=0 leaf output and stale generational lookup |
| `:536`, `projects_ten_thousand_nested_splits_without_call_stack_growth` | Iterative depth/drop behavior and all three per-split slice lengths after input owners are dropped |
| `:1025`, `owned_node_rows_keep_previous_outputs_after_late_geometry_error` | Exact late second-child overflow error versus the oracle, with full old-clone capture after original/constraints/Workspace destruction |
| Existing raw/measurement-priority and override tests | Raw repeated-subtree traversal, malformed input order, minimum/override precedence, and owner/reference validation remain untouched |

The oracle capture independently copies each public extent slice into its own Vec (`layout/reference.rs:38–69`), and the frozen oracle's own solver/traversal produces separate minimum/maximum/actual arrays (`:393–400`, `:495–507`). It has not adopted the slab partition helper, so complete comparison can detect swapped segments, offsets, truncated results, or shared-write corruption. These are appropriate existing qualification targets; this report does not claim a passing execution result.

The seven frozen dependencies named in the base manifest all matched their captured bytes. In particular, the oracle hash remains `2b2eee151d88bdd83048f0dfe7a4931a769aff6616da59e060200819008b209a`, ownership/oracle/depth tests remain `5863123fd6b1733f1ad1e0bfeb0d9f67b6c18e9b20703b9705079e3ae90cf5db`, and the captured benchmark is unchanged by this patch. Later MRU/counter revisions to dependencies are outside these saved files and are not replacement inputs to apply.

This is one complete result-storage change with a clear contiguous-allocation tradeoff. It can proceed directly as the next independent slice against the verified current base. Runtime qualification and any allocation/latency benefit remain Root-owned follow-up work.
