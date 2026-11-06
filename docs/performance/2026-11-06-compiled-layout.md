# Compiled root topology and dense layout execution

Date: 2026-11-06. Baseline: `f28a381499455c252b8772ed772340f6dff9072a`.

## Scope

This change implements the layout part of the [architecture diagnosis](2026-11-06-architecture-root-causes.md).
The docking core remains independent of any renderer. Its public layout result,
constraint solver, stable identities and override validation remain unchanged.
The internal traversal representation changes from keyed intermediate maps to
contiguous structural and measurement rows.

The measured baseline already includes immutable requirement-inventory sharing.
The benchmark workloads and pinned ImGui sources are unchanged. This is a
comparison of one core implementation change, not a Rust/C++ language comparison
or a claim of full docking feature parity with ImGui.

Retain this change for its consistent layout improvement and reduced scene-update
allocations. Do not qualify it as an across-the-board product speedup: full
product idle p50 regressed slightly in the interleaved runs. That result remains
an explicit limitation and a follow-up measurement target.

## Representation and ownership

`RootLayoutTopology` stores node identities and kinds, split axes, ordered child
row indices, persistent weight bits and a depth-first postorder. Compilation
measures nodes as it visits them, retaining the previous traversal's first-error
order. It uses a completed-node map and a visiting set only on the compile path.
The postorder also preserves the existing treatment of completed shared nodes.

Measurements use a contiguous vector indexed by those rows. Repeated child
measurement and projection constraint/weight scratch vectors retain capacity
throughout one operation. Each split still produces its full immutable output
arrays; the result still exposes all three existing ordered maps. The numerical
axis solver, central-area behavior, overflow and splitter compression rules are
unchanged.

Each existing workspace-root index can lazily hold an `Arc<RootLayoutTopology>`
in a `OnceLock`. Cloning the index shares an initialized immutable table. Filling
the table does not change index equality or any authority field. A failed
measurement does not publish a table. Concurrent initialization may compile
twice, but each call returns its own checked result, irrespective of which table
is stored first.

Version equality alone cannot authorize reuse. Before using a cached table, the
core compares the live root, central node, complete generational node identities,
node kinds, split axes, child cardinality and order, and every persistent weight
bit. A mismatch runs the normal complete compile path. A stale same-version hint
does not replace the first stored table. This matching remains linear in the
reachable structure and allocates no traversal sets.

Metrics and leaf answers are always measured again. In the scene-minimum path,
maxima are still completed after bounds are known. Transient weights still pass
the existing source-fingerprint and cardinality checks. The topology is an
acceleration hint and grants no interaction or output authority.

## Measured result

The original scripts ran before and after the change with the same production
profiles and unchanged workloads. Their actual executables were then frozen and
run serially in two before/after ABBA groups. At 1,024 leaves, each layout group
contains 192 samples per condition and case:

| Layout case | Group 1 before → after | Group 2 before → after | Within-group p50 reduction |
| --- | ---: | ---: | ---: |
| Full compilation, measurement and projection | 0.581 → 0.470 ms | 0.584 → 0.465 ms | 19.0–20.3% |
| Projection after preparation | 0.332 → 0.263 ms | 0.331 → 0.263 ms | 20.6–21.0% |

The prepared case's separately timed preparation p50 was 0.230/0.225 ms before
and 0.199/0.199 ms after. Do not add these medians to derive a new percentile;
the full case directly measures the combined work. All node rectangles and
geometry digests match both the old core and the pinned ImGui common subset.
The ImGui in-place routine remains around 7 µs; it updates existing node bounds
and does not construct this core's immutable constraints and split-result maps.
There is still considerable representation and allocation work to remove.

The first standalone product comparison suggested an idle improvement, but the
interleaved executables contradicted it. Two additional reverse-order BAAB
groups retained the same binaries and workloads to check order effects. All
four groups are retained. Each group has 48 samples per condition and scenario.
At 1,024 panes:

| Complete product scenario | Before p50 range | After p50 range | Within-group change |
| --- | ---: | ---: | ---: |
| Idle, one frame | 7.801–8.212 ms | 7.945–8.412 ms | 1.4–2.7% slower |
| Physical tab click, two frames | 29.832–31.032 ms | 29.712–31.039 ms | 0.5% faster to 0.3% slower |
| Surface resize, one frame | 18.795–19.630 ms | 18.350–19.153 ms | 1.5–2.9% faster |

These ranges describe four separate group medians, not a pooled percentile or a
confidence interval. Complete p50/p95/max values are archived for every scale
and group. Tail results are mixed; no general tail-latency improvement is
claimed. The idle regression cannot be explained solely by forward run order.
The investigation has not isolated it to a particular function or established
a cause such as code placement, retained data or reference counting.

### Renderer-free frame attribution

To examine that idle result, the unchanged public core `frame_cost` workload
was run separately. The earlier shared-manifest executable's complete 363-file
input inventory matches this change's baseline. The new executable uses the
same ThinLTO/one-codegen-unit release settings. One ABBA and one BAAB group use
48 samples, six warmups and 1,024 leaves per process, with heap profiling
inactive. The harness exposes process summaries, not individual timings:

| Core update scenario | Four before process p50 values | Four after process p50 values |
| --- | --- | --- |
| Idle | 1.805, 1.841, 1.836, 1.842 ms | 1.839, 1.825, 1.846, 1.860 ms |
| Select one tab | 13.702, 13.838, 13.817, 13.932 ms | 13.327, 13.368, 13.585, 13.665 ms |
| Change measurements | 12.748, 12.821, 12.945, 12.903 ms | 12.258, 12.464, 12.387, 12.254 ms |

Idle update and settlement ranges overlap. The changed-state core update paths
improve, while selection still performs substantially more work than layout
alone. This diagnostic does not identify the cause of the complete-product idle
regression or replace the product measurements.

A separate active-DHAT run uses three samples and one warmup, with the same
complete recording sink and workload. Update-phase p50 allocation counts are:

| Core update scenario | Before | After | Reduction |
| --- | ---: | ---: | ---: |
| Idle | 2,793 | 2,793 | 0 |
| Select one tab | 114,405 | 109,009 | 5,396 (4.7%) |
| Change measurements | 67,856 | 62,390 | 5,466 (8.1%) |

Idle requested bytes are also identical at 17,747,440 per update. Requested
bytes drop by 181,008 for selection and 682,208 for changed measurements.
Settlement allocation counts and requested bytes are unchanged in these three
scenarios. These are allocation requests made during the measured phases,
including the unchanged host recording output, not RSS or a measurement of the
retained topology table's resident footprint. No new idle allocation amplification
is observed; this does not rule out other CPU or memory effects.

## Verification approach

A test-only copy freezes the previous measurement and projection traversals.
Random differential tests compare complete results, including errors, for
multiway splits, unequal weights, central leaves, changing minima/maxima,
fractional bounds, overflow and compressed splitters. Both cold compilation and
warm indexed execution are exercised. Tests also change metrics and answers
while preserving topology, and edit live structure without changing the supplied
version to check that cached rows are rejected.

The renderer-free benchmark includes `project_full` and `project_prepared`.
`project_full` includes fresh topology compilation and measurement on every
sample. `project_prepared` excludes measurement from its projection timer and
reports that cost separately as `preparation_ns`; it does not measure a warm
indexed scene. No preparation cost is hidden or described as eliminated.

The product benchmark uses the egui fork's production configuration, complete
content and generated meshes, and the original idle, physical tab-click and
surface-resize workloads. A tab-click sample contains two frames. These timings
include CPU draw-data generation and exclude a GPU, native window and vsync.

The independent code review found one issue in the test-only oracle wrapper:
the minimums entry checked overrides before measuring, while the old and new
production entry points measure first. A combined-error regression reproduced
the issue, then passed after fixing the wrapper. It covers both cold and warm
topology and preserves the public complete-constraint path's different ordering.
Production code did not change after that review.

The initial complete gates passed 2,150 workspace tests, 292 default-host tests,
335 serde-host tests, 556 native tests and 10 doctests. The opt-in layout probe
and one pre-existing native wall-time case were skipped by their normal suites.
After the oracle correction and new regression, all 1,977 core tests passed;
core Clippy, formatting and whitespace checks were rerun successfully. Workspace
and native correctness/suspicious Clippy and the core dependency check passed
in the complete gate run. Cargo operations used one build job and nextest used
one test thread. No builds, tests, sampling or independent review overlapped the
formal timing windows for this investigation.

The [evidence archive](2026-11-06-compiled-layout/README.md) contains the raw
layout/product samples, forward and reverse comparisons, headless process and
allocation summaries, source/binary hashes, build/check logs, independent review
and its closure. A separate verification script recomputed all layout/product nearest-rank
percentiles and maxima, checked each process's exact sample coverage and raw
rows, and compared every product sample's frame/pass/content/mesh counts with
the baseline. No samples were dropped or substituted.

## Remaining architectural work

This change leaves final ordered-map materialization and per-split result arrays
in place. It also leaves selection on the general workspace transaction path,
the coupling between selection and requirement/index rebuilding, and repeated
scene inventory construction and validation joins. A new index starts with an
empty topology slot, so rebuilding an index after selection does not preserve
the compiled table. The existing exact scene and hit-geometry reuse paths remain
in effect.

Further changes should isolate structural identity from selection state and
share already checked scene rows across internal consumers. Any replacement
must preserve atomic publication, current measurement-dependent layout,
historical output rejection and independent pointer/keyboard semantics.
