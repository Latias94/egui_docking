# Own each projected node result once

Replace `LayoutProjection::node_rects` and `node_constraints` with one
`nodes: NodeMap<NodeResult>`. Each record owns its complete `rect` and
`constraints`. Existing generic lookup, sorted iteration and preorder iteration
remain implemented once. A caller reads `projection.nodes[&node].rect` or
`projection.nodes[&node].constraints`. This is an intentional public API change.

The private map reserves its final capacity and records the complete value at
the existing first Enter rectangle write. Raw completed-subtree revisits
overwrite that identity's earlier record. A failed projection drops its private
partial vector. Successful results and clones independently own every numeric
value and retain the immutable RootStructure, so old outputs remain usable after
the Workspace and original output are dropped.

This removes one node-output buffer per root, placeholder rectangle filling,
the separate constraint-result collection and one topology Arc handle. Required
rectangles and constraints are still fully materialized; the interleaved record
can have different locality from two scalar streams. Measurements, split
outputs, fresh leaf input collection and complete owned export remain real work.
No duration or allocator-byte saving follows from the buffer inventory alone.

The exact reviewed candidate patch is SHA-256
`274634d11c456f95515b8428917eb5f9bd439ec9ae96493165ce4408d1a86232`.
Integration preserves the existing ordinary work-counter/runner suffix in
`docking_cost.rs` byte for byte; all other twelve output files match their
reviewed hashes. The independent oracle changes only its actual-output capture
adapter. Its expected implementation and all numerical reductions, overrides,
error ordering, complete export words and timing boundaries remain unchanged.

The ownership regression retains full clone, sorted/preorder and earlier-output
comparisons. The new late-geometry-error witness records a root and first child,
then rejects the second child's overflowing endpoint while retaining readable
old results. Independent source review found no correctness or scope blocker.

## Qualification and measured limits

Counters-enabled qualification passes 2,065 production core cases, 2,241
workspace cases, four no-default public integrations and 292/335 public host
cases. The three opt-in probes remain skipped in ordinary suites. Focused
ownership/layout cases, four work regressions, formatting,
correctness/suspicious Clippy, strict docs, doctests and dependency checks pass.
The separate ordinary 21-cell work probe retains exactly the RootStructure
completion counts.

The frozen binary SHA-256 is
`140b8162ee98e5ef24df6062ac264acc62848a26bc0b2f5d9dc41fab31da4d5c`.
Complete comparisons use actual RootStructure commit `0b8ab8e` and its frozen
binary, followed by pinned ImGui `d88f238d1d363ef387382b2e29ad12cf96f826ac`.
Each pairing independently runs ABBA and BAAB, 48 samples/eight warmups per
process/cell and 96 observations per condition/group/cell. All original 21
cells retain request, checked mutation, all-root fresh layout and fresh complete
owned export. Complete vectors and digests agree in all runs.

Against RootStructure, the complete-operation p50 reduction range is -11.77%
to +10.55%, with 22 negative group/cells and one tie. At 1024 panes it ranges
from -3.00% to +1.74%. The 128-pane one-view center case regresses 7.65–8.41%
in the two groups; its edge case regresses 11.77% in group two. The full p95
reduction range is -166.10% to +14.29%, with 19 negatives; the max range is
-728.99% to +82.21%, with 23 negatives. This does not establish a stable
hot-path improvement. The checkpoint is retained as a simpler single result
owner and public interface; the measured regressions remain part of its cost.

| Total panes | Complete-operation p50 / ImGui, both groups | Group/cells at most 2x |
| --- | --- | --- |
| 16 | 2.47–5.33x | 0 / 14 |
| 128 | 1.50–2.31x | 7 / 14 |
| 1024 | 1.17–1.61x | 14 / 14 |

The total remains 21/42. Every complete p50/p95 group/cell is slower than ImGui;
31 max ratios are slower. The twofold goal remains active. Phase medians are
not added into a synthetic operation result, and source buffer reductions are
not treated as allocation-byte or latency measurements.

[comparisons.csv](2026-11-07-node-result/comparisons.csv) retains all p50,
p95 and max comparisons. The [evidence archive](2026-11-07-node-result/evidence.tar.xz)
contains 16,128 raw rows, 336 complete vectors, complete build/run/check receipts,
the original reviewed patch, its exact composition record and the thirteen
changed control/candidate file pairs. All 455 inputs were read back and compared
with their original bytes. The unchanged original control remains explicitly
bound to `0b8ab8e`, independent of later documentation or native CI commits.
[Independent evidence review](2026-11-07-node-result/evidence-review.md)
reproduces all comparison rows, checks archived source pairs against immutable
commits and verifies complete vectors, raw samples and retained negative cases.
