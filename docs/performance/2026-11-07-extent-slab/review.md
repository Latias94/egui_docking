# Child extent slab: source and rebase review

Verdict: PASS; no source correctness or rebase blocker found for patch `3cb323c6d799ce448e431c1944d040ada21c7bc4a55fba215cfcd3534eebbfee`.

This review reads the frozen two-file before/after sources, exact patch and receipts, the relevant ownership/oracle tests, and necessary public consumers. It does not scan the whole workspace. No Cargo, compiler, product tests, benchmark, timing, formatting, Git mutation, repository write or candidate modification was performed. Only this report was written. Compilation, behavior qualification and performance remain root's pending gates.

## Exact scope and integration base

The 4,450-byte patch reconstructs exactly from its saved before/after files, including paths, hunk context and line counts. It changes precisely two runtime files, +33/-16. I independently reproduced the sorted two-path set hashes and verified the READY, manifest, scope receipt and README hashes.

| File, relative to `crates/dockspace/src/` | Exact base SHA-256 | Candidate SHA-256 |
| --- | --- | --- |
| `layout.rs` | `325f56f16cacbde46cce531461c8830417a6aa8ff4af06f6c87145a7528963df` | `e4597343ab8b0f8d69f94bebf7e23e190905dace9e77b942b2187c2ef555d26c` |
| `layout/projection.rs` | `d86c008dd78c5f1ed805de76b71d102a29dd3dab4e6ee37ca8f3cb74839edfb0` | `d770e03663941e840e5e37222d9663092c3e699f27b51dee535d301abc492c60` |

The saved base is exactly the checked output of frozen single NodeResult patch `274634d11c456f95515b8428917eb5f9bd439ec9ae96493165ce4408d1a86232`, over the frozen RootStructure candidate and actual `80403cb` control. Both target files in the live repository matched these base bytes at review time. Therefore this patch currently needs no source rebase or fuzzy application.

The saved composition intentionally predates the RootStructure work-counter augmentation. Of the seven hash-bound dependencies, the current `graph/root_structure.rs` differs only by the successful-build hook, and current `docking_cost.rs` differs only by the ordinary CSV's three appended counter columns and values. The other five match exactly. The slab patch touches neither dependency, nor the separate test-only Arc correction. Applying only its two exact hunks preserves all these current additions. Do not overwrite dependency files using the older saved composition.

The earlier slab's complete output is not the base. The present patch does not copy that output, its rejected solver aggregate, or its obsolete test hunk.

## Public slices, initialization and ownership

`projection.rs:203–228` allocates `3 * E` fresh `f64` elements, where `E = topology.children.len()`, and partitions them as minima `[0, E)`, maxima `[E, 2E)` and actual extents `[2E, 3E)`. Two successive safe `split_at` / `split_at_mut` calls return three disjoint, equal-length segments. Construction and derived Clone preserve the length invariant; the private Vec has no resizing or replacement path in this patch. The existing immutable RootStructure remains the source of E.

`projection.rs:285–307` applies the same existing per-split `children` range to each corresponding segment. All three public field names, element values, child order, per-split lengths and borrow lifetimes are preserved. The other SplitProjection fields, map queries, deterministic iteration, Debug and equality implementations are unchanged. Public splitter consumers continue to use the same borrowed slices; no caller migration or compatibility layer is required.

`vec![0.0; 3 * E]` initializes every result scalar to the same positive zero as the old three Vecs. No uninitialized memory, lazy result computation or numerical work omission is introduced. E=0 produces an empty child buffer, the accessors can safely split it at zero, and leaf-only roots still expose no split rows. Raw-DAG child ranges use the same flat edge indexing as before.

`SplitMap` still owns the Vec and derives Clone. Cloning deep-copies all `3E` scalar values while retaining the same immutable structure via Arc; it does not share mutable result storage or borrow scratch. LayoutProjection cloning continues to own both node results and split results. Dropping the original projection, measurement, leaf input or Workspace cannot invalidate the cloned slices or historical outputs. Old outputs are never rewritten by a later projection.

## Solver, traversal and error order

The layout-side changes are only three storage substitutions: borrow the three segment slices for existing minimum/maximum writes, pass the actual segment's original child range to `solve_axis_into`, and load the same solved actual extent during child advancement.

The mutable slice borrow ends after the solver call, before `set_outcome` borrows SplitMap again. No slice or scratch reference escapes the owned result. Minimum/maximum writes retain their original zip order and actual output writes retain their original solver order.

I independently compared all eight claimed unchanged regions, including the entire public solver core, measurement and override paths, fresh single NodeResult construction, splitter geometry, and the complete `validate_inputs`-to-EOF numeric suffix. Their bytes and recorded hashes match the saved base. `validate_inputs` still returns `Result<(), LayoutError>`; desired weights are summed in their original function; both original assigned-size sums remain. The rejected aggregate reuse has not returned. Weight conversion, infeasible minimum/maximum branches, central allocation, grow/shrink, tolerance and final outcome calculations are unchanged.

The continuation frame, pending child size, child DFS schedule and gap creation remain unchanged. Each child finishes before its following gap and next child rectangle are constructed. No bounds are generated early, and no extra numerical operations or different rounding order are introduced. Existing malformed-input, override-before-leaf and minima/override diagnostic precedence is untouched. Construction failures still discard only the private projection under construction.

The new integer `checked_mul(3)` prevents wrapping the buffer length. Allocation remains infallible Vec construction, as in the base. A single contiguous `3E` allocation has a stricter maximum contiguous-size requirement than three separate E allocations and can fail under different allocator/size-class conditions. This is an allocation/capacity boundary, not a new recoverable LayoutError or changed numerical diagnostic. The README states that limit accurately. No impossible-size test or new error framework is warranted for this bounded storage patch.

## Existing distinguishing coverage

The retained registered tests already exercise the changed contract:

- `dense_layout_matches_frozen_traversal` compares complete output to the independent oracle across nonuniform nested/n-ary constraints, central cases, sizes and metrics, including both direct and minimum-only projection routes.
- `dense_projection_keeps_identity_order_and_owned_old_outputs` captures every split field, produces a distinct later output, destroys Workspace and original output, and compares the surviving clone's complete values.
- `owned_node_rows_keep_previous_outputs_after_late_geometry_error` checks the exact second-child overflow error against the independent oracle and verifies surviving complete old output after partial failed projection and owner destruction.
- `sparse_arena_projection_rejects_retired_generations` retains the leaf-only E=0 case and full generational identity checks.
- The registered 10,000-depth test retains iterative complete projection, all three per-split slice lengths, and owned output destruction after Workspace destruction.

The independent capture adapter copies each of the three public slices into separate frozen oracle Vecs; the independent solver and traversal do not adopt the slab helper. Thus full-value comparison can detect segment swaps, bad offsets, truncated extents or corruption. Existing override/raw coverage remains unchanged. No test is added, weakened or removed by this patch; no passing test result is claimed here.

## Cost claims and frozen artifact binding

The source inventory is accurate: for each nonempty root, the three explicit child-result backing requests and corresponding deallocations become one; clones receive the same reduction. All `3E` elements and `24E` payload bytes remain. The primary four-root fixture therefore removes eight such allocation requests; a one-root fixture removes two; a leaf-only root removes none. These statements do not measure allocator calls, memory savings, CPU cost or speedup. Larger contiguous storage and different bounds checks remain empirical questions for root's separate comparison.

| Artifact | SHA-256 |
| --- | --- |
| Patch | `3cb323c6d799ce448e431c1944d040ada21c7bc4a55fba215cfcd3534eebbfee` |
| Two-path base set | `a55a00fcc4d62b6fafa5c294780a629138a57d365cb72f578127b62dc3b42fc2` |
| Two-path candidate set | `862570a7bf610c50607d9e42ddedbab83203a2ba051beadb150131e2c7578647` |
| `base-manifest.json` | `7b106489bca5577156ddce9c00ebb9a9a5ec1f7cf1e4f825a1341526c1e0cefc` |
| `manifest.json` | `66cef3bd21d57f6b6fdfd399c8fef7259fc865a5d3e985ab50952cfbd63a004a` |
| `scope-checks.json` | `5d3ace80ebc68ded74dcf76809b1ae66b03d861528d18e830db242f06dffb4ce` |
| `README.md` | `0b9868fd5806bb845146ac0ea620d36275b222ecf8ff1693c13357b3d1d34caf` |
| `READY.json` | `dfd42eff8514241063e2a9a914fd8d413ba1be01f15ffc14ffdc3c51a831ca15` |
| Unchanged independent oracle | `2b2eee151d88bdd83048f0dfe7a4931a769aff6616da59e060200819008b209a` |
| Unchanged ownership/oracle/depth tests | `5863123fd6b1733f1ad1e0bfeb0d9f67b6c18e9b20703b9705079e3ae90cf5db` |

No source correction or additional regression test is requested before root's normal serial qualification. The original candidate and receipts remain stationary.
