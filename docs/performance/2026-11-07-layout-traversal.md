# Layout traversal state and resize qualification

The layout compiler now uses one node-to-row map and its existing measurement
slots to distinguish active from completed nodes. It no longer maintains a
separate active-path hash set. The complete layout, cycle detection, diagnostic
order and owned projection contract remain the same.

This is a bounded follow-up to the [checked-move refactor](2026-11-06-checked-move.md).
It improves resize p50 in both complete comparison groups at every tested size,
against both the original baseline and the submitted checked-move implementation.
It does not establish a unique cause for the earlier resize shifts or a universal
tail-latency improvement.

## Representation and behavior

Previously, `completed: HashMap<NodeId, usize>` recorded completed rows and
`visiting: HashSet<NodeId>` recorded the active DFS path. The measurement vector
already had `None` for an unfinished row and `Some` for a completed measurement.
The new representation uses that existing state:

| Node state | Representation | Repeated entry |
| --- | --- | --- |
| Unseen | No node-to-row entry | Create its row and begin measurement |
| Active | Row exists; measurement is `None` | Return the original cycle error |
| Completed | Row exists; measurement is `Some` | Reuse the completed subtree |

Both vectors receive their entries before the map stores a row index. Exit still
requires every child measurement. Reversed child scheduling, split cardinality
checks, final completeness and central reachability checks remain in place.
The new regression compares complete results with the frozen traversal for a
shared completed subtree, a later ancestor back-edge and missing leaf constraints.
Shared-subtree acceptance in direct projection is existing behavior; strict
workspace validation still rejects shared ownership.

This removes an active-path collection and its insert/remove hashing. The former
set's size followed traversal depth, not the total node count. Map insertion order
also changes, so the measured gain cannot be attributed solely to one allocation.
Warm topology measurement, exact same-version topology matching, transaction
validation, rollback, the axis solver and projection/export output are unchanged.

## Measurement boundary

The [docking protocol](../../integration/product-comparison/docking.md) is unchanged:
fresh fixture outside timing, then request capture, checked mutation, all-root
layout and fresh complete owned export within timing. Validation, digest and
destruction of the retained projections/output remain outside timing. Every
sample compares the full result with the independent expected state; every
process also writes a complete sample-zero vector.

The matrix fixes total panes at 16/128/1,024 and divides them over one/four logical
views. Four views are roots in one workspace/context, not native OS viewport
lifecycle. Resize alternates the same two bounds. Center, perpendicular edge
split and cross-view commands retain the same source/target fixtures. Rendering,
fonts, frame/window submission, hit testing and host settlement are excluded on
both sides.

The candidate is frozen from the `fa5fb1e` tree plus the two layout-source changes.
Its receipt hashes all 325 original build/protocol inputs. Production uses the
same Rust toolchain, thin LTO and one codegen unit, with test work counters
disabled only in the timing executable. All builds and measurements are serial.
Each comparison has two ABBA/BAAB groups, 48 samples/eight warmups per process,
96 observations per group/condition/cell and 192 pooled observations per condition.
Three separate comparisons retain 24,192 raw observations and 504 full vectors.
Do not subtract their independent percentile values to reconstruct a cost budget.

## Complete-operation results

At 1,024 panes, positive values below mean lower complete-operation p50. The
original baseline is `a2bcabc` with the prior `a9ace58` runtime; the checked-move
baseline is the submitted `fa5fb1e` executable. These are independent comparisons.

| Case | Views | Reduction vs original, groups 1 / 2 | Reduction vs checked move, groups 1 / 2 |
| --- | ---: | ---: | ---: |
| Resize | 1 | 6.86% / 8.32% | 8.58% / 9.23% |
| Center move | 1 | 44.68% / 44.56% | 2.22% / 1.95% |
| Edge split | 1 | 43.53% / 41.77% | 3.01% / 1.65% |
| Resize | 4 | 9.60% / 8.48% | 9.26% / 9.30% |
| Center move | 4 | 28.27% / 28.17% | 1.90% / 1.94% |
| Edge split | 4 | 28.67% / 28.16% | 2.34% / 1.67% |
| Cross-view move | 4 | 27.15% / 27.64% | 1.70% / 0.23% |

All resize cells improve in both groups against both baselines. At 16/128 panes,
resize reductions against checked move range from 1.30% to 7.54%. Against the
original baseline, all move cells also improve in both groups, and both center
shapes retain the required 20% improvement at 1,024 panes. The previous repeated
one-view resize median loss is absent in the final candidate.

The smaller incremental move cells are mixed against checked move. At 16 panes
and four views, cross-view p50 is 20.583 → 20.625 us in group 1 and
21.125 → 21.917 us in group 2. Other 16/128-pane moves have group-2 negative cells,
up to 4.36%; these are retained in the raw data and gate receipt. No claim that
this increment speeds every move shape is made.

Tail values remain mixed. Against the original baseline, four-view 1,024-pane
resize maxima rise in both groups, 647.167 → 809.000 us and
722.167 → 971.791 us. Its p95 rises 514.083 → 530.583 us in group 1 but falls
526.917 → 509.833 us in group 2. Against checked move, that shape's p95 falls in
both groups, while maxima are 773.500 → 812.125 us and
1,469.041 → 865.750 us. At 128 panes/one view, maxima against checked move rise
148.958 → 187.958 us and 118.750 → 150.250 us. All of these observations remain
available. Median qualification is not an upper latency bound or a demonstrated
improvement in every tail statistic.

A larger negative move tail also appears against checked move at 128 panes/four
views: center-move group-2 p95 rises 209.541 → 420.042 us and maximum rises
623.000 → 10,296.042 us. Group 1 improves for this cell. The cause is not isolated;
the observation is retained and is not described as proven scheduling noise.

## Preparation controls and historical limits

The historical formal and resize-only negative results remain in the original
archive. Three additional diagnostics precede the layout change:

- A shortened-history frozen-binary run covers only 1,024 panes. One-view p50
  changes by -0.20% and +0.32% improvement, failing to repeat the earlier direction.
- A repeat of the original all-size resize history uses 96 samples/16 warmups and
  the same ABBA/BAAB ordering. One-view 1,024-pane p50 improves 5.76%/1.42%; four-view
  p50 improves 2.33% in one group and worsens 1.24% in the other. Changing process
  history is therefore not established as the sole cause either.
- A separate executable interleaves current fixture preparation, historical
  canonicalization/strict validation/MRU preparation, and current preparation plus
  an extra historical validator. All modes call the same timed `measure` function.
  Historical preparation is about 0.39%–0.88% slower than current preparation in
  these two repeats, which does not support the simple claim that reverting
  preparation restores the old speed. Extra untimed validation can also change
  subsequent timing; it does not enter the measured resize path.

These controls preserve full output and own 7,296 additional raw observations
and 76 vectors. The setup experiment rotates all six condition orders and balances
overall ordinal counts, but width and ordinal position are not balanced jointly.
It also shares allocator history between modes and uses one fixed rotation.
Historical source algorithms are isolated in private temporary modules, with
wrapper/import changes recorded in the retained sources; its provenance receipt
is narrower than the formal candidate's complete input receipt. It is a bounded
preparation-sensitivity experiment, not an isolated allocator/MRU causal proof.
Its measurements do not replace the formal protocol or resolve the historical
2.08%/0.40% one-view shifts to one unique cause.

The final layout result addresses measured work in the actual resize path. No
additional samples were selected to erase a negative cell, and neither controls
nor unchanged source are used to label all prior losses as noise.

## Remaining difference from ImGui

The third comparison reuses the unchanged real ImGui kernel executable, pinned to
`d88f238d1d363ef387382b2e29ad12cf96f826ac` (1.93.1 WIP). The complete normalized
states agree. Pooled 1,024-pane p50 values from this comparison alone are:

| Case | Views | Rust, us | ImGui, us | Ratio |
| --- | ---: | ---: | ---: | ---: |
| Resize | 1 | 525.458 | 68.792 | 7.64× |
| Center move | 1 | 1,907.875 | 70.000 | 27.26× |
| Edge split | 1 | 2,083.375 | 110.333 | 18.88× |
| Resize | 4 | 424.250 | 67.750 | 6.26× |
| Center move | 4 | 1,634.791 | 69.792 | 23.42× |
| Edge split | 4 | 1,693.000 | 105.375 | 16.07× |
| Cross-view move | 4 | 1,695.083 | 68.875 | 24.61× |

Same-execution phase sums put checked mutation at 64.34% of four-view center
time, layout at 24.74%, request capture at 9.18% and export at 1.74%. These are
shares of raw summed times, not a sum of independent percentile estimates.
The common owned export is a small share of this move workload.

The remaining mutation path still clones the whole workspace for atomic rollback
and performs four global item-multiset scans, plus canonicalization and strict
validation. This increment removes none of those guarantees or passes. A larger
transaction redesign needs one explicit invariant owner and a smaller publication
unit, while retaining staged batch validation, late failure rollback, MRU,
same-version stale proofs, node identity and presentation transitions together.
Layout still constructs richer immutable topology, constraints and split results
than ImGui's in-place update. The ratios measure this docking abstraction boundary,
not rendered product frames, native multiviewport lifecycle or language overhead.

## Verification and evidence

The [implementation plan](../plans/2026-11-06-003-layout-traversal-state.md) records
scope and acceptance. The [evidence bundle](2026-11-07-layout-traversal/) retains
receipts, raw samples, complete sample-zero vectors, source controls, independent
reviews, and verification logs. The ordinary test build restores work counters;
the temporary control module is absent from the submitted crate.

All 2,024 all-feature core tests pass, with three opt-in probes skipped. The four
public-facade integration tests and nine documentation tests pass. Formatting,
dependency boundaries and diff checks pass, as do Clippy correctness/suspicious
checks. Existing all/pedantic style warnings remain; this is not a warning-free
claim. Independent source review finds no blocking implementation issue. The
49 protected user CI/review files retain their starting hashes.
