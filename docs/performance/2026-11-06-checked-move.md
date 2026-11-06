# One checked Move per candidate state

Date: 2026-11-06. Baseline: `a2bcabc1440caa664fb5f4a64c4669c4eecfdce4`.
This implements the [checked-move plan](../plans/2026-11-06-002-checked-move-refactor.md)
against the [matched renderer-free docking workload](2026-11-06-docking-kernel-results.md).

## Implementation and preserved contracts

Move authorization and execution previously each verified the source and
target, with target verification independently re-capturing the same complete
root. The new private operation checks and immediately executes the exact
payload/target while holding the exclusive candidate borrow. Its local current
index materializes only distinct referenced roots and ends before mutation.
There is no checked token, persistent cache, alternate unchecked executor or
new public API.

Each batch command derives its own facts from the actual staged workspace.
Same-root source and target still compare their own full expected fingerprints;
one successful source check cannot authorize an older target. Target capture
still verifies surface, central status, selected-item policy rule and exact
inner/outer scope. Arrangement lock, payload/target semantics and real policy
evaluation precede all no-op and mutation branches. Existing multi-target drag
eligibility keeps its indexed path, and prepared-drop release keeps its current
read-only authorization check.

Exact surface-recovery programs use the same operation and skip only ordinary
policy. The private exact-program/source-roster gates, full current source and
target checks, topology and weight checks remain. New regressions exercise the
real complete-main Tabs and split-Subtree recovery compiler, ordinary policy
rejection, typed recovery success and stale-target rollback. Insertion still
reads parent/index/weights after source detachment; it does not cache mutation
operands from the pre-detach topology.

Canonicalization borrows each node on Enter/Exit instead of cloning it. A split
whose children and axis structure need no rewrite retains its vectors and
weight bits. Actual rewrites allocate their replacement output and preserve
the original composition/normalization order. Strict validation borrows the
node through the immutable workspace reference while updating its bookkeeping.
The iterative stacks still carry IDs, so deep trees do not introduce recursion.
No input, acyclic/root, conservation, sweep, strict-output or presentation pass
is removed.

The second increment replaces complete item collection used only for emptiness
with an iterative early-exit query, and checks exact empty/single/two-item MRU
permutations without temporary sets. Larger stacks retain the existing set
algorithm. Invalid short MRU inputs still use the original diagnostic body;
only a complete successful MRU check returns early. Global duplicate ownership,
selection, empty-noncentral, graph and presentation validation still execute.

The full workspace transaction candidate, four item-multiset scans and final
canonicalization/strict validation remain. This slice reduces redundant
construction and copying while preserving their postconditions; it does not
implement touched-root rollback or incremental command invariants.

## Work attribution

The separate enabled-counter build uses the same probe driver. It resets after
fixture setup and snapshots before external correctness checks; its times are
discarded. Every one of 21 cells matches the new source prediction. For T total
panes and V equally sized roots, the caller still builds its full current
request index. Mutation additionally builds one root for an intra-root move or
two for a cross-root move. Those distinct current values replace six internal
root reconstructions.

| Complete 1,024-pane step | Baseline builds / visits | Candidate builds / visits |
| --- | ---: | ---: |
| One-view center/edge | 7 / 14,329 | 2 / 4,094 |
| Four-view center/edge | 10 / 5,110 | 5 / 2,555 |
| Four-view cross-root | 10 / 5,110 | 6 / 3,066 |

All move rows still have one whole-workspace candidate clone and four complete
item scans, visiting 8,192 item occurrences. The candidate contains 2,047 nodes
at one view or 2,044 at four views, and 2,048 tab items. Resize has zero of these
transaction/proof counters. Per-Node clone removal and short-MRU temporary-set
removal are established by source changes; the ledger does not measure their
allocation event/byte count. No memory saving is inferred from elapsed time.

## First iteration and measurement method

The first candidate already removes repeated root construction and traversal
Node clones. It improves 1,024-pane one-view center p50 by **34.36%/35.80%** in
the two frozen groups. Four-view center improves only **19.14%/18.79%**, below
the plan's 20% investment threshold. Its raw data, source copies and binary
identities remain archived. It is not silently replaced by the second result.
Negative tails are retained: one-view resize maximum is 1.455 ms versus the
baseline's 0.851 ms, and four-view center maximum is 3.869 ms versus 3.507 ms.

The final comparison uses the same frozen baseline and a separately frozen
second candidate. Each group runs ABBA or BAAB serially; each process has eight
warmups and 48 measured samples per cell at 16/128/1,024 total panes and one/four
logical views. Each group/product/cell therefore contains 96 actual samples.
There is no compilation or other root-owned benchmark alongside timing.

The complete endpoint remains request capture, checked transaction, all-root
layout and the same fresh owned state export. No egui/ImGui rendering, text,
tessellation, scene/hit compilation, native OS viewport lifecycle or host
settlement enters this timer. All item order, selection intent, topology and
exact integer geometry are checked. Per-process sample-zero full vectors and
every sample's complete-state digest must agree. All rich Rust projections and
reports stay alive through total stop. Fixture setup, output checks/IO and
result/context destruction are outside; real transaction temporary/replaced
workspace disposal stays inside apply.

The runner and both probe sources are unchanged. The baseline/candidate
comparison is a one-off evidence script using their existing CSV validator and
percentile helper, not an extension of release validation or a new generic
benchmark framework. Rust uses production optimization, ThinLTO, one codegen
unit and one Cargo job, with test structural counters compiled out. The
independent work counter and ordinary tests omit that compile-time variable.

## Final frozen comparison

Microseconds per complete operation. Each group/cell/condition pools 96 raw
samples from two processes. Positive reduction means the final candidate took
less time. Every tail sample is retained, and quantiles use nearest rank.

| Total panes | Views | Case | Group 1 before → after | Reduction | Group 2 before → after | Reduction |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| 16 | 1 | `resize` | 7.125 → 6.583 | 7.61% | 6.625 → 6.875 | -3.77% |
| 16 | 1 | `center_move` | 45.791 → 24.333 | 46.86% | 45.542 → 24.625 | 45.93% |
| 16 | 1 | `edge_split` | 49.041 → 27.791 | 43.33% | 52.208 → 28.000 | 46.37% |
| 16 | 4 | `resize` | 6.750 → 7.041 | -4.31% | 6.833 → 6.875 | -0.61% |
| 16 | 4 | `center_move` | 30.333 → 21.791 | 28.16% | 31.125 → 21.833 | 29.85% |
| 16 | 4 | `edge_split` | 31.958 → 23.417 | 26.73% | 32.416 → 23.583 | 27.25% |
| 16 | 4 | `cross_view_move` | 29.500 → 22.458 | 23.87% | 30.208 → 22.500 | 25.52% |
| 128 | 1 | `resize` | 57.084 → 57.250 | -0.29% | 57.583 → 56.916 | 1.16% |
| 128 | 1 | `center_move` | 372.875 → 194.417 | 47.86% | 370.583 → 194.333 | 47.56% |
| 128 | 1 | `edge_split` | 394.041 → 218.875 | 44.45% | 399.542 → 216.292 | 45.87% |
| 128 | 4 | `resize` | 54.958 → 55.666 | -1.29% | 55.083 → 54.959 | 0.23% |
| 128 | 4 | `center_move` | 253.459 → 177.500 | 29.97% | 253.833 → 177.125 | 30.22% |
| 128 | 4 | `edge_split` | 262.542 → 184.292 | 29.80% | 259.416 → 182.708 | 29.57% |
| 128 | 4 | `cross_view_move` | 255.000 → 181.541 | 28.81% | 254.250 → 184.208 | 27.55% |
| 1,024 | 1 | `resize` | 581.000 → 594.625 | -2.35% | 596.500 → 592.208 | 0.72% |
| 1,024 | 1 | `center_move` | 3483.125 → 1953.500 | 43.92% | 3471.459 → 1986.125 | 42.79% |
| 1,024 | 1 | `edge_split` | 3668.666 → 2158.417 | 41.17% | 3671.375 → 2176.667 | 40.71% |
| 1,024 | 4 | `resize` | 471.125 → 472.000 | -0.19% | 472.375 → 475.333 | -0.63% |
| 1,024 | 4 | `center_move` | 2328.000 → 1676.959 | 27.97% | 2335.250 → 1703.542 | 27.05% |
| 1,024 | 4 | `edge_split` | 2386.166 → 1735.333 | 27.28% | 2399.208 → 1751.833 | 26.98% |
| 1,024 | 4 | `cross_view_move` | 2352.875 → 1749.125 | 25.66% | 2359.959 → 1768.625 | 25.06% |

Both center shapes exceed the plan's 20% criterion in both groups, with every
edge/cross cell improving in both groups and at every size. Resize has mixed
small changes; this is not a resize optimization claim. The 16-pane/four-view
negative cells differ by 0.291/0.042 us. The one-view 1,024-pane resize p50
changes direction between groups. Retain these cells rather than quoting only
the changed-state wins.

The changed-state 1,024-pane p95s/maxima also fall in these final groups, but
resize tails are mixed. One-view resize maximum rises from 0.774 to 1.399 ms in
group 1 and 0.766 to 0.934 ms in group 2; group-1 p95 rises from 0.721 to
0.766 ms while group-2 p95 falls from 0.686 to 0.656 ms. Four-view resize maximum
falls from 0.783 to 0.618 ms in group 1 but rises from 0.744 to 0.839 ms in
group 2. No universal tail improvement or latency bound is established.
At 128 panes/four views, resize maxima rise in both groups: 67.084 → 203.000 us
and 73.834 → 225.042 us. Those negative tails remain in the formal archive.

A single separate resize follow-up reuses the same frozen executables, with
96 samples/16 warmups per process in two ABBA/BAAB groups. Its 1,024-pane
four-view p50s are 508.208 → 503.625 us and 509.333 → 508.292 us, so the small
formal four-view p50 regression does not repeat. The one-view p50s remain
slightly slower: 626.375 → 639.417 us (-2.08%) and 630.083 → 632.625 us (-0.40%).
Their p95s decrease from 843.709 → 784.875 us and 782.375 → 779.375 us, with
mixed maxima. The 128-pane/four-view maxima instead decrease in this follow-up,
195.209 → 182.125 us and 244.500 → 131.209 us. These additional samples do not
replace any formal negative result or establish an unconditional resize
stability guarantee.

Rust resize does not execute the changed transaction, canonicalization or MRU
functions; its measured layout/export source is unchanged. Fresh fixture
construction/validation does change outside the timer, and different executable
code layout, allocator conditioning, caches and scheduling remain possible
influences. This experiment does not isolate their individual contributions.
The residual small one-view resize shift is an explicit limitation, not a
claimed optimization or a proven irreducible cost. No further samples are
selected to erase it.

A subsequent [layout traversal follow-up](2026-11-07-layout-traversal.md) preserves
these results, adds preparation controls and removes duplicate active-path
bookkeeping from the actual layout compiler. Its final complete matrix improves
all resize medians in both groups against both frozen baselines. That later
qualification does not rewrite this checked-move-only result or assign its
historical variation one proven cause.

## Remaining difference from ImGui

A separate final-candidate/ImGui run uses the same renderer-free boundary,
48 samples/eight warmups per process and two serial groups. These are pooled
raw p50s, 192 samples per product/cell; this reference run is separate from the
frozen before/after groups above. The original ImGui probe/source/compiler
settings remain unchanged. All normalized results match.

| 1,024 panes | Views | Final Rust p50, us | ImGui p50, us | Ratio |
| --- | ---: | ---: | ---: | ---: |
| `resize` | 1 | 591.834 | 70.708 | 8.37× |
| `center_move` | 1 | 1997.833 | 72.458 | 27.57× |
| `edge_split` | 1 | 2201.125 | 115.584 | 19.04× |
| `resize` | 4 | 477.416 | 70.417 | 6.78× |
| `center_move` | 4 | 1723.666 | 70.958 | 24.29× |
| `edge_split` | 4 | 1778.375 | 110.958 | 16.03× |
| `cross_view_move` | 4 | 1787.917 | 71.292 | 25.08× |

The actual mutation phase still takes about 62.6–64.2% of summed same-execution
four-view move totals; layout takes about 25.8–26.7%. Phase sums use the same raw
operations and are not sums/subtractions of separate percentile values.
Four-view center's separate phase p50s are request 151.834 us, mutation
1,078.083 us, layout 455.042 us and export 28.708 us; they must not be added to
reconstruct the total p50. The common export does not explain the residual gap.

The next structural issue is still the whole-workspace clone and repeated
conservation/normalization/strict passes. The present result proves meaningful
avoidable overhead in the former implementation, without proving that all
remaining validation can be deleted. A subsequent design should give valid
workspace commands one invariant owner and isolate only the state needed for
atomic publication. Batch staged state, late failures, MRU, node identity,
central/orphan handling and presentation changes must remain correct together.
Removing global passes from this slice simply because the success fixture
matches would not establish those preconditions.

Layout still constructs richer immutable constraints, topology and split
results than ImGui's in-place bounds update; the indexed scene path already
reuses topology, while this standalone full `project_root` benchmark rebuilds
it. These operation ratios are not full rendered product/click ratios, native
OS multiviewport lifecycle ratios or Rust/C++ language ratios. No new product
frame timing or heap/RSS allocation result is claimed by this change.

## Qualification and evidence

All **2,023** all-feature core tests pass in the ordinary production build,
with the three opt-in probes skipped. All four public-facade integration tests
and nine documentation tests pass. Formatting, dependency-boundary, Python
compilation and diff checks pass. Clippy correctness/suspicious checks pass;
all/pedantic style warnings remain, including long regression functions and
test table type complexity. This is not a warning-free Clippy claim.

Independent reviews cover the command design, actual implementation, recovery,
canonical/validator borrowing, second increment, simplification and raw
evidence. No blocking source finding remains. Exact candidate-source hashes
identify each iteration, rather than relying only on the unchanged base commit
label. The first failed-gate iteration and negative resize/tail results remain
visible alongside the qualified transaction result.

The [archive](2026-11-06-checked-move/) contains four complete formal datasets
(first/final before-after and first/final candidate-ImGui), separate resize
follow-up, enabled work counts, exact first-iteration source copies, process
output, all sample-zero vectors, build/environment/source/binary hashes,
verification logs and independent reviews. Executables remain in the recorded
temporary directories and are not committed. Source/reference and executable
hashes are checked before accepting each measured result. User CI/review files
retain their 49 starting hashes and are excluded from the commit.
