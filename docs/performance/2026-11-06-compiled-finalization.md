# Finalizing compiler-owned surface plans

Date: 2026-11-06. Baseline: `aba4c27b00746de929329fac0a0db3054448a7f6`.
This follows [shared scene row orders](2026-11-06-scene-row-order.md).

## Why a different interface is justified

The host supplies measurements, not arbitrary pane/tab/splitter records.
Production scene records are constructed inside the core compiler. Previously,
ordinary contributions and native first-live projections each compiled a raw
plan, then ran the general validator, which independently reconstructed much
of what that compiler had just emitted.

That general reconstruction is useful for deliberately malformed test plans.
It need not be the production interface for a closed compiler. The new
`CompiledSurfacePlan` is an opaque, consuming result issued only by
`scene_compiler/surface.rs`. Its private fields retain the plan and immutable
borrows of the compiler's workspace, manifest and policy, plus the exact version
and contained projection. It has no mutable-plan or cloning interface.

`finalize()` consumes the object. It creates the checker from those captured
inputs, rather than accepting a separately selected checker, and runs the
geometry checks, canonicalization and linker. `into_finalization()` consumes
the qualification and yields the raw plan with its bound checker; it cannot
reissue a compiler-owned result after arbitrary mutation. The production raw
compiler kernels are private. The old raw measurement entry is test-only.

The existing engine and native projection owners still establish correct
workspace/manifest/index pairing. The new wrapper retains their immutable
inputs; it does not infer source provenance from version equality. The live
same-version topology guard, measurement validation and publication checks
remain in place. No extra cache, provenance history or runtime trusted flag is
introduced, and no old workspace is retained by a new owned `Arc`.

## Removed reconstruction and retained checks

| Removed production reconstruction | Construction that supplies the fact |
| --- | --- |
| Expected pane roster, selection and layer map | Each projected tabs node is visited once, and its pane takes selection and layer directly from that source context |
| Expected splitter-gap and rendered-splitter sets | Each projected split emits its indexed gaps; the same intersection decision emits the corresponding rendered handle |
| A second junction derivation and expected-junction map | The compiler derives checked candidates once after all splitters, rejects ambiguous/empty hit bands and computes operability against the complete occlusion roster |
| Expected magnetic root/pane maps, depth traversal, edge identities and field coverage | Root, pane, target and field construction use the same loop variables and checked geometry factories; fields are not supplied by the host |

The general validator retains all four reconstructions for arbitrary plans.
The remaining production checks are shared with it: ticket and exact answer
sets, authoritative measurements, bounds and popup state, index/version checks,
contained projection source, uniqueness, pane containment, bar/tab/member and
control/menu relationships, contained/menu geometry, splitter arrays and scalar
constraints, semantic membership and operability, target/guide geometry and
identity, actual target coverage, occlusions, and contained minima. Availability canonicalization,
canonical ordering and actual surface-program linking still run.

The ordinary and native paths now both use this constructor/finalizer pair.
The reducer's later stamp, policy, coordinates, contained projection, popup and
native lifecycle checks are unchanged. A compiler result certifies construction
for its input snapshot; it does not authorize publication after that state
changes.

## Checks that cannot simply be removed

Independent architecture review identified two valid external-input cases in
which raw compilation can succeed while the resulting geometry must be
rejected. New regression tests execute both cases:

- An empty protected central leaf with a 1,000-point leading reservation and
  a 100-point surface can produce a zero-width viewport located outside the
  bar. Non-negative width alone does not prove containment.
- With `tab_min_width = tab_max_width = 8`, a member can be completely visible
  while close chrome leaves no operable tab record. Member/record consistency
  must still be checked.

The new finalizer rejects these with the same `InvalidTabBarRecord` as the
general validator. Another regression preserves rejection of a contained
projection with an incorrect root or stale source rectangle. These are actual
checks in the consuming finalizer, not test-only assertions. Geometry failures
remain validation errors in the existing contribution error category.

Implementation review found another accepted-input counterexample: two
adjacent contained windows can jointly occlude an empty central pane whose
content height is zero. Guide geometry is omitted, while the compiler falls
back to emitting a center target from pane bounds. No magnetic pane field is
emitted for zero content, leaving that target uncovered. The original general
validator rejects it. The initial candidate omitted this relationship check;
it was fixed before timing or publication. Finalization now marks targets from
the actual emitted root/pane fields in the already existing guide coverage
directory and requires complete coverage. This preserves rejection without
reconstructing field geometry, expected depth or expected pane inventories.
The exact two-window case is a new differential regression.

## Correctness evidence

Four new tests cover the two external geometry counterexamples, contained
projection source guards, and a generated comparison of complete results.
The property test varies measured bounds, minima, intrinsic widths,
reservations, tab width configuration, visibility policy, arrangement lock and
coordinate scales through `1e100`. Its workspace has a fixed nested split with
an optional empty central leaf and a contained root; it does not generate
arbitrary forest topology; the separate joint-occlusion regression adds two
contained windows. It compares compilation failures as well as full
canonical plans or finalization errors. Existing tests cover popup, clipping,
resize, native projection, rollback, measurements and malformed raw plans.

A temporary experiment hook also ran the complete validator frozen from
`aba4c27` against the exact same input and index/projection mode. In 564 selected
scene/native/tab-strip tests, 339 generic-validation results and 789 compiled-
finalization results matched the frozen complete result or concrete error.
The hook was removed with exact source-byte verification before final gates
and timing. It is retained only as compressed experiment evidence.

Final-source checks and independent implementation review are recorded in the
[evidence archive](2026-11-06-compiled-finalization/README.md).

All 2,170 workspace tests, 292 default-host tests, 335 serde-host tests, 556
native tests and 10 doctests pass after the coverage fix. The 64 focused tests
are included in the workspace count. The normal opt-in layout probe and
existing native wall-time case remain skipped. Workspace/native correctness
and suspicious Clippy checks, formatting, dependency boundaries and whitespace
checks pass. The initial implementation review's P2 coverage finding was fixed
and independently rechecked; follow-up review found no remaining blocker.
Reviewers did not run tests or benchmarks. Earlier pre-fix receipts are archived
separately and are not used as final qualification.

## Performance qualification

The baseline executables are the previous experiment's frozen candidates.
Their hashes and complete source inventory match committed `aba4c27`. Benchmark
sources and pinned ImGui inputs remain unchanged. Cargo runs with one job and
existing targets. Product timing uses the existing production ThinLTO profile;
renderer-free timing uses release ThinLTO and one codegen unit. Timing is
serial, separate from builds, tests, review and active heap profiling.

The complete 16/128/1,024-pane workloads still measure all required answers,
compile full surface output, execute ordered paint recording, accept its
receipt, commit and settle output. Product before/after measurements use ABBA
and BAAB, 24 samples and six warmups per process. Renderer-free measurements
use the same orders with 48 samples and six warmups. Separate active-DHAT
runs use six samples and two warmups.

Each product order group pools actual samples from two processes per condition,
giving 48 samples per case. At 1,024 panes:

| Scenario | Group 1 before → after p50 | Change | Group 2 before → after p50 | Change |
| --- | ---: | ---: | ---: | ---: |
| Idle, one frame | 8.120 → 8.273 ms | +1.9% | 8.271 → 8.189 ms | -1.0% |
| Physical tab click, two frames | 28.406 → 27.427 ms | -3.4% | 28.474 → 27.448 ms | -3.6% |
| Surface resize, one frame | 18.707 → 17.708 ms | -5.3% | 18.727 → 17.753 ms | -5.2% |

Idle does not show a benefit. Every before/after sample has identical frame,
pass, content, visible-content, vertex, index and draw-command counts. All
scales and every p95/max remain in the archive. Click p95 improves in both
groups, but group 2 candidate click maximum is 31.734 ms against baseline
29.213 ms; no general maximum-latency improvement is claimed.

The separate unchanged whole-product reference comparison records candidate
click p50 27.204 ms versus pinned ImGui 2.278 ms, about 11.9 times. Idle is
8.057 ms versus 1.145 ms. This large remaining gap includes two frameworks'
complete rendering and different draw-command workloads. It is not a pure
docking-core ratio and is not erased by this implementation.

At 1,024 panes the renderer-free update has these four process p50 values per
condition:

| Scenario | Four baseline process p50 values | Four candidate process p50 values |
| --- | --- | --- |
| idle | 1.872, 1.859, 1.892, 1.897 ms | 1.887, 1.896, 1.889, 1.887 ms |
| select | 11.101, 11.115, 11.117, 11.136 ms | 9.984, 10.036, 9.949, 10.027 ms |
| drag | 13.762, 13.917, 13.835, 13.802 ms | 12.627, 12.775, 12.711, 12.757 ms |
| measurement_change | 12.307, 12.397, 12.353, 12.437 ms | 11.557, 11.318, 11.317, 11.217 ms |
| outputs_in_flight | 11.049, 11.185, 11.109, 11.111 ms | 9.967, 10.091, 10.019, 10.086 ms |

The selection median improves by approximately 10%, and all candidate process
medians for each scene-invalidating case fall below every baseline median for
that case. These are individual process summaries, not pooled sample
percentiles. The example does not retain individual timing samples. It selects
through a semantic action, distinct from the product's two-frame pointer click.

Tail variation remains visible: selection candidate group 2 run 1 reaches
19.033 ms, and measurement-change candidate group 1 run 2 reaches 18.676 ms.
Retained-output candidates also have maxima of 16.601 and 17.081 ms. Every
process p95/max and settlement summary is retained; median improvement is not
a bound on any future frame.

Separate active-DHAT allocation accounting at 1,024 panes is:

| Update scenario | Events before → after | Requested bytes before → after |
| --- | ---: | ---: |
| select | 73,348 → 68,716 | 49,726,356 → 46,127,996 |
| drag | 67,309 → 62,679 | 75,746,068 → 72,152,844 |
| measurement_change | 59,387 → 54,755 | 75,010,348 → 71,411,988 |
| outputs_in_flight | 73,367 → 68,735 | 49,729,428 → 46,131,068 |

Selection removes 4,632 allocation events (6.3%) and 3,598,360 requested bytes
(7.2%). Idle and every settlement's event/byte counts remain unchanged at all
scales. Live-before/live-after and case-prefix peak accounting are identical
for every case at every scale. This reduces temporary allocation traffic,
not retained heap or RSS. Active-profile timings do not qualify latency.

The candidate is retained for consistent median improvements with the complete
output workload and lower allocation traffic. Neutral idle results and all
tail regressions are preserved alongside the gains.

## The remaining gap

This changes the ownership of construction guarantees, not the complete scene
output contract. A local selection still triggers full-surface construction,
and measured bounds, minima, tab chrome and policy can require new geometry.
The compiler, target/guide processing, hit/semantic manifests, host recording
and framework rendering remain substantial work.

The larger architectural opportunity is to reuse unchanged geometry while
rebinding the current semantic authority. The previous painted plan is already
retained as a stale fallback, but its availability for computation does not
grant current input or publication authority. Any reuse must compare the actual
dependencies: selected minima and tab-bar policy, measurements, scroll/reveal
state, popup, resize overrides and contained projections. Whole-root command
fingerprints also change with selection/MRU, so retaining geometry must not
silently retain stale target sources. The new restricted construction interface
provides a smaller place to establish such a reuse rule.

The complete product comparison includes two frameworks' layout, widget and
mesh work. It cannot assign the whole ImGui ratio to the docking algorithm.
Likewise, complete-output complexity bounds cannot prove the current elapsed
time is irreducible. The next useful evidence is the cost of invalidating an
entire surface after a local edit, alongside framework cost attribution, rather
than a claim that Rust ownership or validation has an unavoidable millisecond
floor.
