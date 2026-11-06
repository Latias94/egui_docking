# Dense complete projection: independent source review

Reviewed at 2026-11-06T17:40:50.160973+00:00. This review is bound to all eleven files and the READY hash in `/tmp/dockspace-imgui-twofold-20261107-ea4161e/layout-implementation.md`. Each file matched both before and after inspection; combined hash `fda937c7676d3ee3c2391eeaa3ef3b6fe5622c2763c289411c1f22b7e9a74bf6`. The frozen conservative arena slice is reviewed separately in `arena-review.md`. The concurrent core owner implementation is outside this verdict except for reading its `WorkspaceData`/Builder access signatures needed by layout. No Cargo, build, Rust execution, tests, timing, measurement, profiler, repository edit, Git write or child agent was used. This report is the only layout-review write.

**Verdict: one definite compile blocker in a migrated test caller; no second confirmed runtime-correctness blocker found in the dense representation/solver change. Fix the missing borrow, then compile and execute the existing bounded layout/oracle/splitter tests before claiming qualification. The earlier 2,024 passing tests and arena timings do not qualify this new layout implementation.**

## Blocking finding

**[P1] Borrow the new split view at the existing test validation call.**

At `crates/dockspace/src/engine/splitter_geometry/tests.rs:567`, `projected.splits.get(&split).unwrap()` now returns a `SplitProjection<'_>` value. At line 571 this value is passed directly to `validate_split_projection`, whose second parameter remains `&SplitProjection<'_>` at `engine/splitter_geometry.rs:334`. A free-function argument does not auto-borrow a value. This is an expected E0308 type mismatch in the library test build, even though rustfmt succeeds.

The minimal correction is only the argument:

```diff
-                    resized,
+                    &resized,
```

The two production calls in `splitter_geometry.rs` already apply the corresponding `&split` migration. There are no other callers of `validate_split_projection` or `intersect_split_intervals` in the reviewed tree. This review did not apply the fix.

## Interface, identity and ownership

`NodeMap<T>` uses dense traversal values plus a shared immutable root-local `RootLayoutTopology`. `row` binary-searches the identity-sorted directory and compares the complete `NodeId`, including generation; it does not use the arena index alone. A key from a retired generation or another root cannot acquire a value by sharing a slot index. Every directory entry originates in a successful `WorkspaceData::node` lookup in compilation (`topology.rs:82–91`).

The map constructor is private and receives exactly one value per topology row. Public callers can read but cannot mutate the internal vectors or topology. The unchecked-by-Result indexing in map getters and iterators follows these compiler-local length/range invariants; no new unsafe code is introduced. `keys`, `values`, forward/reverse iteration and logical equality visit the same full identity order as the former BTreeMaps. Equality deliberately compares key/value results rather than Arc identity or traversal order.

All three output owners hold an Arc to immutable topology. New projections own fresh numeric vectors and clone them on projection clone. `SplitProjection<'a>` is a Copy borrowed view into the owning `SplitMap`, not a workspace borrow or independently mutable answer. Returning an old projection after dropping the workspace/index remains valid. The topology contains flat vectors and no child ownership recursion, so dropping a deep result does not recursively traverse its graph.

The consumer changes preserve ownership at the meaningful seams: scene records copy extents into `Arc<[f64]>`; resize gesture groups copy borrowed slices into owned Vecs; preview/menu code reads node bounds from NodeMap. No target authorization, semantic fingerprint, leaf requirement, policy or pointer lifecycle is bypassed by the container migration. This review does not imply authorization of separate concurrent core changes.

## Dense split ranges and traversal

During topology Exit each unique split appends its direct-child rows and weights and receives disjoint child, splitter and outcome ranges. The child-edge arrays can include repeated child row indices in raw DAG diagnostics, but ranges belong to split owners rather than child IDs. `SplitMap` initializes arrays from those total edge/gap counts, and each access uses the corresponding ranges stored on that split row.

The `split` ordinal is assigned in postorder before `split_rows` is reordered for identity iteration. This is intentional and consistent: outcomes use the stored postorder ordinal, while iteration uses sorted split rows and then each row's own ordinal. Rebuilding the iteration directory does not require rewriting outcome ordinals. The outcome count remains the number of unique splits.

Projection uses the same Enter/ContinueSplit DFS schedule and cursor arithmetic as before. It writes final child sizes directly into the split owner's range; the continuation holds indices, bounds/cursor and one pending size. Scratch constraints/weights are not retained across recursive continuations. Ancestors' extents cannot be overwritten by descendants because their split ranges are disjoint. Successful completion visits every reachable node and fills every gap; placeholder values remain unobservable on an error because the entire projection is discarded.

Raw completed DAG subtrees remain handled by unique measurement rows but **projection still visits every edge occurrence**, overwriting the shared node's final bounds and split result on a later occurrence. This preserves the former map's last-write behavior. A repeated same split cannot be active within itself in an accepted topology because that would already be a cycle; sequential repeated occurrences cannot corrupt a still-active instance of that same split. The existing completed-subtree/cycle oracle regression continues to exercise this path through a layout-private raw-Builder adapter.

A zero-child raw split is rejected by composition before output allocation. Singleton raw splits have zero gap count and retain the old single-child solver behavior. Valid public workspaces retain their stronger canonical-tree requirements.

## Solver arithmetic and error ordering

The new internal solver writes into an exactly sized caller slice. Public `solve_axis` still validates first and returns owned `AxisLayout`; projection validates the same inputs before writing. Both call sites establish slice length from weights/child cardinality. Min-overflow and max-saturation paths write every element; desired-size paths fill every element before summing. `eligible` scratch is cleared at the start of each redistribution pass, and no early-return path reads its stale contents. It therefore cannot leak the prior split's child indices into the next split.

The extraction preserves arithmetic and summation order: sibling scaling, central remainder, clamp order, assigned sums, central grow/shrink priority, weighted redistribution, stable residue consumption, gap compression and residual tolerance are unchanged. It introduces no float precision conversion, epsilon change, binary-only assumption or omitted direct-child constraint. Range writes replace Vec collection without moving child ordering.

Cold compile still measures as it traverses: early missing leaf answers precede later malformed/cyclic topology, not a separate structural pass that changes priority. Output sorting happens only after successful compile/central checks. Public projection validates root/override source before leaf measurement; the internal scene-minimum path measures before override validation, as before. Numeric outcome is stored before descendant projection now, instead of inserting a completed split at Exit, but partial results are not published on failure and successful raw-DAG last-write behavior remains equivalent.

Root-local storage is O(reachable nodes + edges + gaps), independent of workspace arena slot capacity. It still owns a root-local HashMap during cold compilation and an identity sort, and warm reuse still performs exact structural matching/current measurement. Raw DAG traversal can repeat edge paths exactly as before; this is not a new guarantee of DAG projection complexity. No performance result or twofold goal claim follows from this source review alone.

## Oracle and test quality

The frozen reference owns its own old BTreeMap/Vec result types, traversal and numerical implementation. The capture adapter copies actual output into those old value types; expected results do not flow through the new dense writer. I independently compared the explicit old numerical functions against read-only `git show ea4161e:crates/dockspace/src/layout.rs`: `solve_axis`, `desired_sizes`, grow/shrink, redistribution, capacity, bounded-delta, checked-sum and allocation-tolerance bodies match after whitespace normalization. The copied input checks also retain the same branches/order. Composition and geometry helpers shared with the reference are unchanged in this slice.

The new randomized numerical comparison checks complete values and exact float bits. Full projection differential tests compare all rectangles, composed constraints, direct-child minima/maxima/extents, splitters, central index and residuals. New deterministic cases exercise ordered iteration, two outputs alive with changed bounds/metrics, projection use after workspace/index drop, sparse/recycled generations and missing-node versus earlier missing-leaf priority. The existing ten-thousand-depth integration case remains and exercises the iterative path.

The old same-version hint test's impossible committed-missing-node mutation is moved to a raw draft diagnostic test because Workspace is being sealed. Valid weight/axis/central/order changes still test structural hint rejection/retention after Builder publication. Deliberately invalid DAG/cycle tests use a cfg(test) adapter that calls the real compile/projection implementation; they do not create invalid sealed production Workspaces. No expected numeric outputs were relaxed in the integration/splitter migrations.

No additional general test/oracle framework is needed. Root's next gate should compile the combined owner/layout candidate, then execute the existing layout module/property/raw-diagnostic cases, `layout_solver`, engine splitter/surface-reuse checks and normal serial core qualification. Exact full-output frozen measurement remains necessary afterward; no timer or output boundary change is included in these eleven files.

## Reviewed source hashes

| File | SHA-256 |
| --- | --- |
| `crates/dockspace/src/drop_resolver/preview.rs` | `103826cd4cbfbb00f8b210ec7c6f9d9523114dab6ff428efaf3049d7aeeb9905` |
| `crates/dockspace/src/engine/splitter_geometry.rs` | `bdb69158fa29c8ba2621081264973a45cea4c2d2370d41813eb79aa28f90d9de` |
| `crates/dockspace/src/engine/splitter_geometry/tests.rs` | `baff61a23542ec9c3453d6b1df886c039277d9bef279740319e9810c187267a8` |
| `crates/dockspace/src/layout.rs` | `ec44b2ddd4ac9bc529222de44950f1cc6c1e2cd894d357e7c6593c23ba95bca9` |
| `crates/dockspace/src/layout/override_tests.rs` | `021ef5012a4ff7bae53c57407c22ec18f58beaaa333e98e6b833500f8d6e0676` |
| `crates/dockspace/src/layout/projection.rs` | `c31bfdad6ec84a8714f1d93fa93f97fdb96cb96226349263018da09c62dd742e` |
| `crates/dockspace/src/layout/reference.rs` | `93afdf6e8ad79d81f2efdafc4814a4bb6076fd9f535129f82ce33f9ee12ddf4e` |
| `crates/dockspace/src/layout/topology.rs` | `6776c07f2ea7172aa3a41d95e2f6ecd93bc4efcac0fc18a4d5f1144a21dd49c4` |
| `crates/dockspace/src/layout/topology_tests.rs` | `0b7293be7358ad66ccb268aaec619e290541d595ce4bd7111c3b82ec672f6c94` |
| `crates/dockspace/src/scene_compiler/surface.rs` | `f1b18633f701241a2bd476abe91aee60c01362861ca7e0ce4bede9ec4f82539c` |
| `crates/dockspace/tests/layout_solver.rs` | `b64d99a54be54f719531b2bfb0962fb322764ee0a28cd87dd690f520fbc611d0` |

These hashes describe the reviewed READY state with the missing borrow still present. Applying that minimal fix changes the corresponding file/combined hash and must be recorded before freezing a candidate. This review is not a compilation receipt.
