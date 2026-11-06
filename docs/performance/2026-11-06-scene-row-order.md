# Sharing scene row orders through validation

Date: 2026-11-06. Baseline: `7f959df126701f06ce037386b3caccfce746e6d0`.
This follows [immutable presentation fact reuse](2026-11-06-structure-reuse.md).

## Problem and change

The validator independently sorted pane, tab-bar, tab, splitter-gap and splitter
IDs to find duplicates, built ordered maps for semantic joins, and then sorted
the actual records again before linking the surface program. Tab validation
also allocated a per-bar visible-record vector, a visible-item hash set and a
hidden-item vector. Member-to-visible-record lookup was linear for every member.

Each of those five record classes now has one private `RecordOrder<K>` holding
sorted `(identity, original row)` entries. It detects the smallest duplicate,
supports semantic lookups and sorted iteration, and supplies the final record
permutation. All checks still run. Records keep their original input order until
every check succeeds; only then is the directory consumed to permute records
in place, without another sort or a second permutation allocation. The surface
program links against the same final canonical record tables as before.

Tabs for one bar occupy a contiguous range of the sorted tab directory. Two
partition searches locate that range. Binary search within it replaces both
the visible-item set and member-to-visible-record linear lookup. Hidden items
are compared as an iterator in workspace item order rather than materialized
into a temporary vector. Tab records are checked in directory order, but an
error retains the earliest invalid original row within the original bar. This
preserves diagnostic precedence even when input records are shuffled or more
than one tab is invalid.

The pane-policy map is unnecessary after each pane has already matched its
expected selection and layer. Queries use that checked selected item directly.
Splitter and rendered-gap sets are compared as sorted iterators instead of
constructing more sets from the already sorted rows. Expected structural
splitters and junctions are still independently derived.

The directories exist for one validation call. They add no cross-frame cache,
invalidation state, workspace provenance or public interface. Synthetic test
plans and internally compiled plans use the same validator. This change does
not add a compiler-trusted flag or remove post-compilation geometry checks.

## Correctness evidence

Four new tests cover the row permutation against an independently sorted record
oracle, smallest-duplicate detection, canonical complete-plan equality after
independent permutations of all five record classes, duplicate class priority,
and original error order when multiple visible tabs are invalid. The property
test includes empty inputs, arbitrary keys, duplicates and unique permutations;
the complete-plan tests preserve and compare the linked surface program too.

An experiment-only test hook ran the complete validator frozen from `7f959df`
against the exact same input as the candidate. Its helper recreated the same
complete/shared-surface index mode and contained projection, then compared the
whole `Result<PresentationPlan, SceneBuildError>`. In 560 selected scene,
native and tab-strip tests, all 668 comparisons matched. This includes complete
accepted plans and concrete error values. It is a test-suite differential, not
an exhaustive enumeration of all possible workspaces or floating-point inputs.

The frozen source and wrapper are archived for inspection. The hook was removed
before final tests and measurements, with an exact reconstruction check against
the saved candidate bytes. No comparison code or extra validator executes in
production. Formatting and a test-only cleanup after the comparison are
recorded in the final source inventory.

All 2,166 workspace tests, 292 default-host tests, 335 serde-host tests, 556
native tests and 10 doctests pass on the final source snapshot. The 60 focused
tests are included in the workspace total. Normal suites skip the opt-in layout
probe and existing native wall-time case. Workspace/native correctness and
suspicious Clippy checks, formatting, dependency boundaries and whitespace
checks pass.

Independent static review found no blocking defect or changed validation/error
semantics. It confirmed a coverage limitation: the new complete-plan permutation
test starts with linked records, so it proves restoration of those existing
slots. The existing compiler tests cover initial linking from an unlinked
construction program. The four new tests do not independently enumerate every
arbitrary permutation before its first link. The reviewer did not run the
tests or benchmarks.

## Performance qualification

The [evidence archive](2026-11-06-scene-row-order/README.md) records the exact
source inventory, frozen executable provenance, all product samples, every
renderer-free process summary, separate allocation accounting and final checks.

The complete existing workloads remain unchanged: 16/128/1,024 panes, two tabs
per pane, full measured contributions, ordered paint recording, receipt
acceptance, frame commit and output settlement. Product timing uses the
production ThinLTO profile; renderer-free timing uses release ThinLTO and one
codegen unit. Cargo uses one job and existing targets. Timing runs serially,
separate from builds, tests, review and active heap profiling.

The baseline executables are the previous experiment's frozen candidate
executables; their hashes and full source inventories match committed
`7f959df`. The candidate changes only the three recorded source/test paths.
Benchmark sources and pinned ImGui sources are byte-identical. Both frozen
Rust executables run one ABBA and one BAAB group. Each product process uses
24 samples and six warmups; pooling the two processes per condition within
each group gives 48 actual samples per case. At 1,024 panes:

| Scenario | Group 1 before → after p50 | Change | Group 2 before → after p50 | Change |
| --- | ---: | ---: | ---: | ---: |
| Idle, one frame | 8.169 → 8.245 ms | +0.9% | 8.230 → 8.190 ms | -0.5% |
| Physical tab click, two frames | 28.871 → 28.515 ms | -1.2% | 28.690 → 28.487 ms | -0.7% |
| Surface resize, one frame | 18.834 → 18.670 ms | -0.9% | 18.965 → 18.629 ms | -1.8% |

Click and resize medians improve modestly in both groups. This is not a general
tail-latency improvement: group 1 click p95 increases from 29.668 to 32.174 ms
and its maximum from 29.827 to 34.949 ms. Candidate idle also has a 14.508 ms
maximum against baseline 9.539 ms in that group. Every scale's raw samples,
p95 and maximum remain in the archive. Frames, passes, content calls, visible
content, vertices, indices and draw-command counts match before/after for every
sample. Idle does not establish a benefit from changing the validator.

The unchanged reference comparison still has a large whole-product gap:
candidate 1,024-pane click p50 is 28.088 ms and pinned ImGui is 2.243 ms. Both
include complete rendering in their respective frameworks and produce
different draw-command workloads. This ratio cannot attribute the remaining
gap solely to docking logic or establish a lower bound for the core.

The renderer-free workload also uses ABBA/BAAB, with 48 samples and six warmups
per process and inactive DHAT. It selects through a semantic action, rather
than the product's two-frame physical pointer click. At 1,024 panes, the four
process p50 values per condition are:

| Update scenario | Four baseline process p50 values | Four candidate process p50 values |
| --- | --- | --- |
| idle | 1.879, 1.914, 1.917, 1.898 ms | 1.910, 1.880, 1.883, 1.866 ms |
| select | 11.430, 11.495, 11.338, 11.402 ms | 11.059, 11.058, 11.055, 11.054 ms |
| drag | 14.081, 14.230, 14.100, 14.233 ms | 13.702, 13.688, 13.768, 13.848 ms |
| measurement_change | 12.669, 12.707, 12.692, 12.701 ms | 12.365, 12.305, 12.343, 12.374 ms |
| outputs_in_flight | 11.355, 11.559, 11.355, 11.474 ms | 11.118, 11.003, 11.144, 11.121 ms |

For each scene-invalidating scenario, every candidate process median is below
every baseline process median. These are process summaries, not pooled sample
percentiles; the example does not expose individual sample times. Tail variation
remains: candidate measurement-change group 1 run 2 has a 15.972 ms maximum,
and retained-output group 2 run 1 has a 15.690 ms maximum. All process p95/max
and settlement results are retained.

Separate active-DHAT runs use six samples and two warmups. At 1,024 panes:

| Update scenario | Allocation events before → after | Requested bytes before → after |
| --- | ---: | ---: |
| select | 76,351 → 73,348 | 50,508,388 → 49,726,356 |
| drag | 70,312 → 67,309 | 76,528,100 → 75,746,068 |
| measurement_change | 62,390 → 59,387 | 75,792,380 → 75,010,348 |
| outputs_in_flight | 76,370 → 73,367 | 50,511,460 → 49,729,428 |

Each of these updates saves 3,003 allocation events and 782,032 requested
bytes. For selection this is a 3.9% event reduction and 1.5% requested-byte
reduction. Idle and settlement event/byte counts remain unchanged at every
scale. Live-before/live-after and case-prefix peak accounting remain identical
in all cases, including the 1,024-pane selection peak of 31,871,904 bytes. This
removes transient allocation requests without establishing a retained-heap or
RSS reduction. Active-profile timings do not qualify latency.

The candidate is retained for consistent renderer-free median improvement,
lower allocation traffic and small product-median gains. The archive preserves
the tail regressions and unchanged-path variation alongside those gains.

The row-directory change reduces repeated sorting and storage, rather than
making full scene validation constant-time. For `R` records, one `O(R log R)`
sort supplies duplicate detection, lookup order and an `O(R)` final permutation.
For a bar with `M` members and `V` visible records, member lookup changes from
`O(M * V)` to `O(M * log(V + 2))`. The hidden-item check also uses binary lookup
instead of a hash set; it trades an expected-linear hash pass for deterministic
searches with no per-bar temporary storage. The unchanged two-tab benchmark is
not a scaling measurement of a wide single tab bar.

## Remaining architectural work

This removes repeated data preparation while preserving the complete validation
contract. It does not settle which production checks are necessary. The
construction seam described in the previous report remains relevant: a host
supplies measurements rather than arbitrary scene records, while the internal
compiler creates identities and relationships later checked by the validator.
Moving a proven invariant into a restricted constructor can eventually remove
a redundant check, but a boolean asserting that the compiler is trusted would
not establish that proof.

The remaining passes include expected-pane traversal, expected-splitter and
junction derivation, magnetic-field verification, target validation,
availability canonicalization and surface-program linking. The first
requirement refresh also still captures root fingerprints independently in
semantic snapshots and the fresh index. Each is a concrete next seam to measure
and simplify. The current elapsed time is not a theoretical lower bound.
