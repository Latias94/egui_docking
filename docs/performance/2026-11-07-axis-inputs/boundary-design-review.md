Findings first: **do not replace the committed projection call with an unconditional `solve_validated_axis_into`.** A valid LogicalRect can have an infinite width/height even though both corners are finite. The existing extent-finiteness error is reachable with a valid committed graph. **A narrower removal of repeated graph/weight validation is justified by the existing ownership boundary**, while retaining extent checks at their current position and the same numeric executor. This is worth a bounded experiment; no performance result is established and the overall goal remains 28/42.

The premise also overcounts the current scan: `validate_inputs` at [layout.rs:1142](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/layout.rs:1142) checks extent, lengths, central index, each weight, weight sum and zero total. It does **not** scan constraint minima/maxima. `AxisConstraint::new` checks those at construction; min/max aggregate checks are inside the existing numeric executor. There is no repeated constraint-scalar loop here to remove.

Only this report was written. No patch, Cargo, compilation, tests, benchmark, timing, runner, network, repository/fork/cache/candidate mutation or delegation was performed. Counterexamples below are source deductions, not executed results. HEAD was `edaa96b6a0eb6b9465b8f09455a215167d56be5c`; the inspected layout includes the slab WIP, identified separately by hashes below. No implementation candidate was reviewed.

**A concrete committed-path counterexample.**

[geometry.rs:79](/Users/frankorz/codes/rust/egui_docking/crates/dockspace/src/geometry.rs:79) makes LogicalPoint coordinates finite. `LogicalRect::from_min_max` checks only ordering (`:163-167`); `width`/`height` subtract the corners (`:215-222`). Let M be `f64::MAX`. Construct corners (-M, 0) and (M, 10) through `LogicalPoint::new`, then their rectangle through `from_min_max`. Both constructors succeed, but width is positive infinity. The analogous vertical rectangle has infinite height. This uses ordinary geometry constructors, with no malformed graph, unsafe code or raw test mutation.

For a committed horizontal two-Tabs split, normalized positive weights, zero splitter thickness and leaf constraints with zero minima and finite unit maxima, full `project_root` reaches `solve_axis_into` with infinite available extent. Today it returns `NonFiniteExtent { value: +infinity }`. Bypassing the entire validator reaches the finite `max_total < extent` branch of the executor, assigns unit sizes and produces infinite unallocated extent. Child rectangles at x=-M with unit sizes can round to equal finite corners and need not rescue the error. The essential divergence already occurs at the executor's returned outcome. Graph validity supplies no bound on caller geometry.

This also limits the Constraints type argument: `LogicalRect::size` constructs a LogicalSize directly from those differences (`geometry.rs:227-232`), and `Constraints::new` checks ordering without rechecking finiteness (`:522-525`). Thus a public geometry-derived size, and even point Constraints made from it, can contain infinity. Do not describe every Constraints getter as unconditionally finite. This review does not propose changing geometry constructors or their accepted domain.

Projection's existing postorder composition still checks the relevant scalar arithmetic. A split's used-axis child minima/maxima are summed through the existing checked composition, including splitter extent; perpendicular minima/maxima keep their max/min folds and LogicalSize construction (`layout.rs:798-876`). A non-finite used-axis leaf bound fails there before geometry. Keep those operations and errors. The proposed bypass must be justified by actual preparation and owner invariants, not an overstated geometry-type theorem.

**Closed graph and weight writer inventory.**

The source supports positive finite, cardinality-correct, normalized weights for every published committed split:

| Boundary/writer | Source evidence and surviving contract |
|---|---|
| Scalar and convenience constructors | `graph.rs:51-129`: SplitWeight's field is crate-private; `new` rejects non-finite and nonpositive f32, and normalization rechecks each rounded result, rejecting underflow to zero. `Node::split` checks at least two children and matching normalized weights (`:254-276`). Constructor success alone need not prove final normalized tolerance; publication still validates. |
| Builder / canonical publication | Workspace storage is private; mutable Builder is a distinct type, and `into_builder` consumes Workspace (`graph.rs:416-444,609-647`). `build_workspace` runs canonicalization before issuance (`canonical.rs:223-227`). Input weight/cardinality checks are at `:689-714,755-769`; complete output validation is at `:491-493`. `validation.rs:672-713` requires at least two children, matching cardinality, each strictly positive finite weight, and normalized sum. |
| Declarative model / import / serde | `model/layout.rs:248-267` normalizes model weights; assembly uses Builder (`:623-627`). Persistence stores raw f32 DTO weights (`persistence.rs:193-199`), scalar-checks them (`:622-629`), constructs checked SplitWeights (`:773-786`), then validates and builds the candidate (`:458-477`). Parsing a DTO does not issue a Workspace. Strict import errors must remain. |
| Resize command | `operation.rs:1940-1988` verifies the complete source, split kind, duplicate order, matching cardinality and normalized sum; only validated typed values are copied (`:1992-2020`). A caller can construct positive but non-normalized weights, so this command-level normalization check remains necessary. |
| Same-axis edge insertion / wrapper | `operation.rs:3382-3396` constructs both positive finite replacement weights before writing; wrapping constructs fraction/complement through `SplitWeight::new` (`:3409-3423`). Rounding/underflow can fail and must continue to fail. |
| Child removal / flattening | Removal deletes child and weight together, assigns a checked unit singleton or normalizes survivors (`operation.rs:3148-3171`). Flattening multiplies old shares and calls checked normalization before replacement (`canonical.rs:625-679`); a product underflow is an error, not an allowed zero. |
| Transaction publication | General and owned drafts canonicalize before publication (`operation.rs:811-829,846-879`). The single-Item shortcut validates inserted and final live output splits before completing rows (`canonical.rs:239-255,299-335`). Selection changes only Tabs; other topology transfers retain existing split scalars or pass through these constructors/rewrites. Failed discardable drafts do not become completed Workspace output. |

I inventoried production SplitWeight construction/assignment and split-weight mutation sites across these modules; direct invalid tuple construction is confined to diagnostic test fixtures. Public `Node` assembly can supply non-normalized positive weights to a Builder, but cannot bypass its publication checks. No new validator or “trusted” marker is needed.

`SharedSplit` borrows the graph's actual Arc<Node> weights (`graph/root_structure.rs:14-42`); children are built from that same payload (`:172-199`). NodeStore mutation uses `Arc::make_mut` or installs a new Arc (`graph/shared_storage.rs:35-46`). The completed body's payload cannot silently mutate underneath preparation, and Editing roots have no structure read view (`graph/authority.rs:102-106`). This is actual owner storage, not an independently synchronized weight cache.

Consequently, at committed prepare:

- Weight/constraint/output cardinalities agree: constraints are gathered once per actual child, extents use that child range, and payload weights have the checked graph cardinality.
- `central_index` comes from ordered `position` on those children and is in range. No guessed index is admitted.
- Durable f32-to-f64 conversion preserves strictly positive finite scalars. The normalized sum is finite and near one, so repeated per-weight finite/nonnegative checks, the validation-only sum and ZeroWeightTotal cannot fail.
- Validated overrides retain the same guarantees: `validate_split_weight_overrides` (`layout.rs:668-728`) preserves root/source/duplicate/split/cardinality order, converts typed SplitWeights, and explicitly rejects non-finite or non-normalized sum. Keep that validator unchanged, including its exact generational source checks.

Normalization permits tolerance rather than exact equality to one. Preserve the actual sum/division, central sibling scaling, clamping and redistribution in the executor; substituting a constant total of one would change numeric results. Only its earlier validation-only weight scan/sum is redundant here.

**Checks and error ordering that must survive.**

Retain finite/negative available-extent checks immediately after `projected_splitter_geometry`, where `validate_inputs` currently begins. Negative extent is ruled out by ordered corners and splitter compression on this path, but retaining both O(1) checks is the smallest conservative boundary. Finiteness is not redundant. Do not move them to the root entry: measurement, override and splitter-extent errors presently have priority.

Finite LayoutMetrics does not prove finite total splitter extent: three children with thickness M require two separators whose sum overflows. `total_splitter_extent` (`layout.rs:879-895`) and composition must retain the current `NonFiniteSplitterExtent` diagnostic. Actual child/gap `LogicalRect::new`, cursor advancement and late errors must stay in DFS positions. Valid finite root bounds can still encounter overflow when infeasible minima extend children past those bounds; the existing late-error/old-output witness is `layout/topology_tests.rs:1025`.

Leave `solve_validated_axis_into` and its callees unchanged: min/max aggregate checks, infeasible minima/maxima outcomes, sibling-weight and desired-size checks, assigned-size sums, tolerance and redistribution. Some checks may already be constrained by earlier composition in the committed path; they also serve the arbitrary-input solver and are part of one existing numeric implementation. Removing them or inventing a second executor is outside this slice. For example, two individually valid `AxisConstraint(0,M)` maxima can overflow the public solver's maximum aggregate. Finite scalar types do not generally imply finite arithmetic results.

Keep full validation for the pub-declared `solve_axis`, raw drafts and the frozen reference. Here “pub-declared” describes the existing implementation contract: graph/layout are private production modules, so this is not an assertion about an externally exposed application API. The arbitrary-f64 solver intentionally accepts individual zero weights, allows all-zero weights with a central child, and rejects all-zero nonempty weights without central. It validates before infeasible-minimum early return (`tests/layout_solver.rs:147-215`). Those contracts differ from strictly positive graph weights.

Raw `compile_draft` is not a committed graph validator: it checks traversal/active cycles/cardinality and measures children, but can carry deliberately invalid weight scalars into geometry (`layout/topology.rs:19-133`). Preserve its full axis validator at each actual split Enter, including completed-DAG revisits. Eagerly checking every raw weight before geometry could make a later NaN mask an earlier child/gap geometry failure. The existing exact validator order is extent, arity, central index, first bad weight, aggregate weight, zero total. The reference's independent validation (`layout/reference.rs:673-714`) must remain checked.

Full projection validates overrides before fresh leaf reads (`layout.rs:491-510`). Delayed minimum measurement can fail before overrides exist; consuming projection validates overrides before bounds-dependent maxima (`:552-575,602-630`). In particular, infinite rect extents can fail during the minimum-only maximum stage with Geometry rather than the later full-projection NonFiniteExtent. Preserve these distinct schedules and fresh reads.

**One recommendation: a bounded static boundary, retaining scalar geometry checks.**

A private const generic threaded only through the existing `project_measurements -> project_subtree -> prepare_split_projection` chain is proportionate if it does exactly this: committed entry points, including delayed projection, select the committed preparation; cfg(test) `project_draft` selects full checked preparation. Both call the same unchanged `solve_validated_axis_into`. Committed preparation still performs the current extent checks at the same location, while raw preparation invokes the complete existing validator. No persistent flag, topology mutation, trust cache, provider or proof framework is required.

RootStructure's Rust type alone cannot select the route because raw compilation creates the same type. Private entry wiring is the boundary. The const parameter must not become a user-selectable option or spread into composition, solver arithmetic, graph storage or frame types. It creates two traversal instantiations in tests, not two numeric algorithms. A global `cfg(test)` choice that makes every test use the checked route is weaker: tests would then miss the actual committed path being changed. Both routes should execute in ordinary test builds.

This small static distinction is preferable to adding a runtime trust field or duplicating the DFS merely to avoid a boolean in private signatures. An unconditional unchecked call is smaller textually but fails the concrete geometry contract above. If static separation expands beyond these three layers, reconsider scope. The source opportunity is one per-weight scalar scan and one validation-only sum per projected split; f32 conversion, constraints gathering, all numerical passes and complete output/export remain. Cost and whole-operation benefit are unknown.

Minimum necessary qualification is focused:

1. Add the committed finite-corners/infinite-extent witness on both axes, asserting the exact full-projection error and the existing delayed-minimum ordering; compare against the independent reference, without changing constructors to reject that previously accepted rectangle.
2. Preserve the public solver zero/central/empty/arity/aggregate contracts and existing solver/writer/import/override tests. Exercise ordinary committed projection through the false specialization; keep raw diagnostics through true. Reuse current dense/reference, minimum/override and extreme-weight coverage rather than building another validation framework.
3. Add only missing raw priority witnesses: an earlier child/gap geometry failure versus later NaN weights, and the reverse reachable order. Retain raw DAG/cycle/cardinality/missing-leaf behavior, old complete outputs after failure, and full fresh leaf-read sequence.

Root remains the execution owner. No source approval of a future patch, test success or performance claim follows from this analysis.

Source SHA256 identities (paths below relative to `crates/dockspace/`; fixed for this review):

| Source | SHA256 |
|---|---|
| `src/layout.rs` | `e4597343ab8b0f8d69f94bebf7e23e190905dace9e77b942b2187c2ef555d26c` |
| `src/layout/topology.rs` | `d2d06077b9a5bfb03f764b0ed9c862de4e9aca3a65946059e480b8412f356f56` |
| `src/layout/reference.rs` | `2b2eee151d88bdd83048f0dfe7a4931a769aff6616da59e060200819008b209a` |
| `src/layout/projection.rs` | `d770e03663941e840e5e37222d9663092c3e699f27b51dee535d301abc492c60` |
| `src/geometry.rs` | `3f4912b953fbb5a39b12f718370be8b133354f7a155609896156680d3d7b6541` |
| `src/graph.rs` | `dde95ddfa9e733448a4565404d9b6a612a3f614735bc39ba2cc4233066b80c2a` |
| `src/graph/shared_storage.rs` | `18c6f75150643779acb9c71c30486634b19f7ccec5677e4cc64ce9043fcc532e` |
| `src/graph/root_structure.rs` | `e80a74d1bdef525bd25a9a627bd1c826f41cd1def8fe05583dcea14372938632` |
| `src/graph/authority.rs` | `89d0d337cdc2365caf62f5dd2d48d6f9132b21c2d4b882e9c24096cc6602a600` |
| `src/canonical.rs` | `dda19b95f3a8c29aca4e262b9b4239e1409ca8aceaf85ef2c3432c881aea79be` |
| `src/validation.rs` | `bcbc2b79d3da167b101b40ae1683986d44f0b58bdcf3c7e865a64df081c02d79` |
| `src/operation.rs` | `b18c0f29b0561af1bbed80600d72d94825d5fb5ac1708abf1af7fe864d15c0b5` |
| `src/persistence.rs` | `bb30a2a4d3383ed633c7a42e2c5d7ddc9246573c12b08a48a569bc5059437b34` |
| `src/model/layout.rs` | `ef44caca8d5eb1594dbecd27f73c99a142dac8366d5404790871bd7a05fd2711` |
| `tests/layout_solver.rs` | `4b750cd3f85457f26a86529011b5e8396ea04a0dcd46648180969d55943014ee` |
| `src/lib.rs` | `d9df921215f65048e3ff4a7963e316c733c145671b909506a202f903d87f08d0` |
