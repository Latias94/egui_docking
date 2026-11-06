# Complete output reads in the projection's existing tree order

This candidate exposes `NodeMap::iter_preorder`, a borrowed, exact-size,
double-ended and fused iterator over the immutable dense rows already retained
by a complete projection. Sorted identity queries, iteration, equality and
debug behavior remain unchanged. The method adds no allocation, directory,
Workspace borrow or preprocessing.

The complete exporter still traverses the actual Workspace, reads its root,
child order, axis, ordered items and selection, and emits the original full
header/item sequence into a new owned Vec. It consumes one projected row per
actual node, verifies the complete generational NodeId, and checks root
exhaustion. It removes each node's binary rectangle lookup and one duplicate
Workspace node lookup. Every semantic read, finite/integral check, path
calculation, output word, allocation and ID/exhaustion check stays inside the
original export timer. The rich projection and normalized export keep their
original ownership and destruction boundaries.

The real resize-frontier reader uses the same seam while retaining its existing
source, membership, incident, constraint, split/gap and interval checks. A
changed descendant order cannot borrow old positions merely because equal child
extents happen to fit. The full-ID preorder comparison rejects that mismatch.
Selection or weights alone do not reorder IDs; their existing checks still
govern admission.

Registered tests compare every fresh exported word with both the preserved old
keyed exporter and the independent expected-state oracle for all primary
operations at one/four views. Sparse/recycled/n-ary/central tests independently
check preorder, full identities, mixed front/back iteration and repeated
exhaustion while retaining sorted-query coverage. Old results are read after
creating another output and dropping the Workspace. The production resize test
uses a fresh source and a retained projection with equal-size descendants
swapped, exercising geometry correspondence rather than trivial stale authority.
The solver and frozen reference are unchanged.

Counters-enabled checks pass 78 focused and 2,226 workspace tests, formatting
and correctness/suspicious Clippy. Independent source review finds no blocker
in output completeness, order admission or timer preservation. The solver and
public signatures are unchanged; this slice does not claim a separate rerun of
the production full suite or strict documentation. The frozen production probe
checks the original complete matrix and owned outputs.

Both frozen comparisons preserve every complete vector and sampled digest. At
1,024 panes, complete-operation p50 improves 15.06–30.89% against `5ea7fbb`.
Export phase medians are about 13.8–14.2 microseconds instead of the previous
53–75 microseconds; layout remains about 108–114 microseconds and move mutation
about 78–81 microseconds in that paired group's separate phase medians. The
phase medians are not an additive percentile budget.

Of 42 group/cell medians, 41 improve and one 16-pane center cell regresses 0.88%.
Five p95 and nine max cells regress, with largest regressions 225.18% and
475.81%. All negative observations remain published. The result does not
establish tail stability.

| Total panes | Complete-operation p50 / ImGui, all cells and groups | Cells at or below 2x |
| --- | --- | --- |
| 16 | 4.07–8.80x | 0/14 |
| 128 | 2.10–4.41x | 0/14 |
| 1,024 | 1.74–2.96x | 8/14 |

The 1,024-pane resize and perpendicular-edge cells pass the twofold threshold
in both groups. Center/cross-view moves and every smaller-pane cell remain
above it. This is the first partial threshold result, not goal completion.
Next isolate prepared-report ownership and the remaining full candidate copy;
small-root layout allocation/structural overhead also needs a real experiment.

The [archive](2026-11-07-export-reader/evidence.tar.xz) contains 411 input files,
16,128 raw rows and 336 complete vectors, exact source/binary/check receipts,
the reconstructible patch and source review. Every member was read back byte
for byte. [Comparisons](2026-11-07-export-reader/comparisons.csv) retain all
p50/p95/max cells and [SHA256SUMS](2026-11-07-export-reader/SHA256SUMS) identifies
the artifacts. The rejected solver-aggregate patch is absent from this source.
