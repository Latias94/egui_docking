# Independent source review: one owning NodeMap<NodeResult>

Verdict: **PASS for source integration; no confirmed correctness, scope, or qualification-evidence blocker found in the exact frozen patch.** This implements the smaller single-map Interface without preserving the two obsolete scalar-map shapes. Root remains the sole owner of compilation, registered tests, native qualification and controlled measurements. No test or performance result is claimed here.

Reviewed patch SHA-256: `274634d11c456f95515b8428917eb5f9bd439ec9ae96493165ce4408d1a86232`, 43,492 bytes, 13 paths. This review wrote only `/tmp/node-result-single-map/review.md`; no source/candidate/repository edits, Cargo, Rust execution, tests, benchmarks, timing, network, Git writes or sub-agents were used. Hash comparison, source reads and in-memory patch/source composition are the only checks performed.

## Frozen source and scope binding

The base is exact commit `80403cb4bfe66f65a48da490a2aae660091f5f06` plus the 16 frozen RootStructure files from patch `8dc1059d026310e0ef26a9ff5bcb1bca92cd8d00cff62a0bcf833a0c1404ad0b`. I independently read the commit blobs and overlaid the frozen files in memory. All 13 saved base files match that declared origin exactly: four use RootStructure overlay bytes and nine use the original commit blobs. No live-workspace file was substituted for a declared base.

| Bound artifact | SHA-256 |
| --- | --- |
| Complete patch | `274634d11c456f95515b8428917eb5f9bd439ec9ae96493165ce4408d1a86232` |
| Runtime subset | `06d1c4a28648dc7ad359ace612706ebc57b5367e3e6f411ea3471bac674ffa6f` |
| Caller subset | `762781a9c6f87c672e7002c7c7f7c15e8a4824e05ed4cc03755c2de57d9e74fa` |
| `candidate-hashes.json` | `c1f703833e0e884db06993d69e91066a2dd613644dece151004ac88cbd0c5b39` |
| `base-receipt.json` | `77411105f0a84b2c6703b1a98bac66e8d9f391156994b9c4ffd1d1fce891d2c3` |
| `README.md` | `a89bb4a4013a10c53f149fcf79d5dbfb8ff662c1157fea44a990e9b2526d877e` |
| `receipt.json` | `10a9fe089e143c16703061791f6bd484b28e713439b264b0c2d82e25e7858d70` |
| Sorted base path set | `5eb4713e2edc42a8fd123ee8c5200e28f5cfcb88670899cc91ad5e97ae057237` |
| Sorted candidate path set | `81aa437a0de40c3f443fe52beca559bb47faa618cddb0ac90d50654133a574ac` |

Both sorted set hashes reproduce using the documented ascending UTF-8 path + NUL + file SHA + newline format. All 13 individual base/output hashes, all declared artifact hashes and patch byte sizes match. Exact hunk context/count reconstruction in memory produces all 13 candidate outputs byte-for-byte; no patch was applied. The source change is +297/-185, net +112, including tests; the two runtime files are net +17.

## Interface and completion invariant

`layout/projection.rs:13` exposes the complete record `NodeResult { rect, constraints }`, and `LayoutProjection::nodes` owns one `NodeMap<NodeResult>`. Exact generational lookup, sorted and preorder iteration, reverse/exact/fused iterators, Index, IntoIterator, Debug and value equality continue through the one existing map Implementation. The backing Vec becomes private. There is no parallel view family, selector trait, provider, compatibility alias or second topology owner.

The constructor at `layout.rs:633` reserves capacity for the immutable structure's node count. It does not initialize placeholder rectangles or collect a second constraint-result Vec. The only record call is `layout.rs:970`, at the original rectangle write location in `prepare_split_projection`: it stores the supplied current bounds and the already completed `measurements[row].constraints` together.

The complete composed source has exactly one production NodeMap construction and one record call. No public constructor or mutable values accessor can publish a partially filled map. `project_measurements` returns the owner only after `project_subtree` succeeds (`layout.rs:636`), followed by the complete-cardinality debug assertion. On error, its private partial Vec is dropped. The debug assertion is not a replacement for a removed runtime validator: traversal and immutable row construction already establish the required ordering and reachability.

## First-entry append and completed raw-subtree reuse

The `record` rule (`layout/projection.rs:36`) appends when `row == len`, otherwise overwrites an already present row. Source tracing establishes why it does not encounter a future unfilled row on a successful supported path:

- Production `RootStructure::build` assigns node rows on left-to-right first Enter. It constructs direct child-row joins from those same child lists. Projection follows those immutable lists in the same DFS order. Each first Enter therefore reaches the next unused row.
- The test-only raw compiler also assigns rows on first Enter. It skips a repeated node only after its value is complete; active reentry is rejected as a cycle before projection. The entire first traversal of a completed shared subtree has already assigned all its descendant rows.
- Projection may traverse that completed raw subtree again under another parent. Every row encountered on that repeated traversal is already stored; overwriting preserves the previous final-visit rectangle semantics. The copied constraint is the same completed measurement associated with that exact row. Once traversal resumes into a never-before-seen branch, its first row is again the next append position.

This preserves the raw DAG oracle's last-rectangle-write behavior without duplicate map entries. First-visit preorder remains the immutable row order, not the number or order of later overwrites. The active-cycle, missing-node, cardinality, central and leaf-measurement diagnostics remain in the unchanged raw compiler. A new sparse array, validity flag, fallback map or extra validator is unnecessary for this invariant.

The existing registered completed-subtree/cycle/leaf-error test still compares full actual output with the independent raw oracle. The sparse/recycled-generation and mixed front/back iterator tests remain present and migrate to the complete records.

## Owned outputs and errors

Each successful projection owns its numeric `Vec<NodeResult>`. Clone copies that Vec and the existing split result arrays, sharing only immutable RootStructure handles. Public record fields do not expose mutable access through an immutable NodeMap. Dropping or editing the Workspace, dropping another projection, or failing a later projection cannot mutate an existing output's numeric records.

The old ownership test keeps its complete frozen value comparison, identity order, reverse/preorder behavior and different-second-output check. Its extension holds a direct borrow of the cloned map, drops Workspace/index/original output, then compares complete rect and constraint maps and complete preorder records. This exercises the changed owning representation rather than merely testing derived Clone syntax.

The late-error test at `layout/topology_tests.rs:1025` is a valid source-level witness. Each fixed leaf requires 0.375*f64::MAX width. Their total minimum/maximum is 0.75*MAX, which is finite. The first origin-zero, MAX-width projection succeeds. With later bounds origin 0.5*MAX and width 0.5*MAX, minima force the two 0.375*MAX leaf extents: the first endpoint is 0.875*MAX and finite, while the second endpoint is 1.25*MAX and overflows. The root and first leaf have already been recorded before the second child's bounds fail. The expected error remains the exact `GeometryError::NonFinite { component: "x", value: INFINITY }`; the independent oracle must return the same error. A previously owned clone/map borrow remains readable after the original and inputs are dropped.

Record construction adds no new fallible geometry operation and changes no arithmetic order. Public override checks still precede fresh leaf measurement where they did before; minimum-only measurement still completes current constraints/maxima at the same stage. Split allocation, child DFS, intervening gap construction and subsequent child bounds retain their original ordering. The rejected solver-aggregate candidate is not restored.

## Callers and benchmark completeness

I independently scanned all **561 Rust source paths** in the exact commit + overlay + candidate composition, including paths outside the author's narrower crates/integration scan. The only remaining `node_rects` / `node_constraints` names are the independent oracle maps and assertions reading those expected values, in `layout/reference.rs` and `layout/topology_tests.rs`. No old production field read, scalar-view family or missed old NodeMap constructor remains in that composed source. This is source coverage, not a typecheck.

The important migrations preserve their previous checks:

- **Scene and future layout:** sorted scene iteration and exact target/child lookups now select `result.rect`; policy, clipping and MissingNodeProjection branches are retained. Preview center and payload projection use the same lookup/error order.
- **Splitter geometry:** the preorder record's complete NodeId is compared with the expected actual graph identity before its rect/constraints are read (`engine/splitter_geometry.rs:131`). Reusing the same record removes a redundant lookup; it does not authorize a reordered live graph or omit the trailing iterator exhaustion check. Split extents, weights, constraints and rejection tests retain their values.
- **Retained facts tests:** actual minima reads migrate to `result.constraints` without changing the expected retained behavior.
- **Public/diagnostic tests:** scalar reads and now-identical cardinality assertions migrate to one map. Every record necessarily contains both fields, so checking this single map's node count is equivalent to the former duplicated field-map counts.
- **Independent oracle:** only `capture` changes its production-output Adapter, collecting the two independent expected BTreeMaps from the full records. Its expected types, measurement, traversal, solver, arithmetic and owned split values remain separate and unchanged.

`docking_cost::export_state` (`docking_cost.rs:327`) still constructs a fresh owned `Vec<u64>` for the complete state. The new standard iterator map selects `&result.rect` from the same complete preorder; it does not allocate another intermediate Vec or filter nodes. `append_node` still requires each next exact NodeId to equal the actual DFS node (`:285`), writes the same view/path/kind/bounds/tab-order/selected-intent words, recursively visits every actual child, and checks iterator exhaustion for each root. The keyed export regression is retained.

`measure` (`docking_cost.rs:439`) is byte-identical: request capture, mutation, all-root actual projection, fresh owned export, black boxes and every timing boundary remain inside the same measured operation. Neither fixture work nor required export/measurement work is moved out of timing. The complete formal and ordinary work CSV/runner suffix is unchanged; later root-owned counter additions must be composed explicitly rather than overwritten with saved whole files.

## Independent frozen-region checks

The following ten source regions were independently compared against base bytes and their declared hashes: public/caller-owned solver core; override/measurement/constraint/traversal block; split preparation after the record statement; splitter geometry/DFS continuation/axis math; complete existing NodeMap public query/iterator Implementation; complete SplitMap Implementation; frozen independent oracle Implementation; fixture/request/all-root layout helpers; complete measure timing function; and formal/ordinary CSV/runner suffix. All match exactly.

Relevant explicit identities:

- Independent oracle Implementation: `a94d7bafd8d07c4bcfeb934a36c86676baed83e54e4eaf944a7b01bc932b22b3` (25,121 bytes). Only its capture Adapter outside this region changes.
- Unchanged RootStructure definition/build: `3fb52047760880503ef65bb5cc76a6a0ece5024608c5488a73eef3e11b552bde`.
- Unchanged measurement/raw compiler: `d2d06077b9a5bfb03f764b0ed9c862de4e9aca3a65946059e480b8412f356f56`.
- Unchanged geometry: `3f4912b953fbb5a39b12f718370be8b133354f7a155609896156680d3d7b6541`.
- Unchanged override tests: `f18d00fac60f87cd8fe136df5eb25bb36ffe444bda606ed16db4571568ae7fcb`.

All 16 preexisting top-level topology-test functions are retained; the candidate has 17. The four RootStructure-specific added tests are byte-identical to the overlay. The real 10k-depth case remains registered with only direct result-field/count migration. No numeric tolerance, policy, complete output comparison or expected diagnostic was weakened.

## Cost and final disposition

The Interface is materially simpler than the earlier dual-view candidate: one owning map and record type, one query/iterator family, and no compatibility layer. This removes one node-result backing Vec and one retained topology Arc handle per nonempty output, the placeholder rectangle fill, and a separate constraint-result collection. It preserves all required numeric results and fresh measurement/export work.

Those source facts do not establish a measured improvement. Interleaving rects and constraints changes access stride and allocator behavior; a raw repeated subtree now recopies its completed constraints when overwriting the full record. Actual padding, allocations, peak memory, cache effects and paired timings remain root's measurement questions. The README qualifies these costs appropriately.

No candidate correction is requested by this review. Root can integrate the exact patch, preserve any newer overlapping counters/layout candidates by explicit hunks, and perform its planned serial typecheck, numeric/error/ownership/export/splitter/retained-facts suites and controlled measurements. Source is stationary from this review's perspective; this report is the only new artifact.
