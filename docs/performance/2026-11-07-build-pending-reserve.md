# Reject bounded initial graph-build stack reservation

The current-source diagnosis identifies actual pending-stack growth during
timed Item edge completion. This experiment reserves the existing standard
`Vec<BuildFrame>` using `node_capacity_hint.clamp(1, 16)`, then pushes the
same initial Enter. It passes correctness checks but fails the declared
affected-workload performance rule. The initializer is restored exactly;
retained runtime `0e441c8` and its 30/42 formal target inventory remain.

The actual source/binary control is `c7508ee` /
`3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`.
Candidate binary:
`e67648635ff6edf1b2c61d26b0bd78d153fc4f5ba04e105246f483981c8ae2bf`.
The previous-nodes hint, initial frame, every push/pop/extend, checked lookup,
directory operation, placement callback and error position remain unchanged.
Sixteen is an initial reservation, not a depth limit: deep/nary trees retain
normal Vec growth. A shallow or collapsed root can overreserve; this is a
real cost, not claimed free memory. The sole source change is the initializer,
not a container replacement or a new structural traversal.

All twelve normal counter-enabled gates pass, including 432 targeted tests,
2073 production core, 2249 workspace, four public integration, 292/335
independent-host and ten documentation tests, plus formatting, strict docs,
dependency boundaries and correctness/suspicious Clippy. Existing deep-tree,
local/general normalization, empty-source, rollback, generation/old-reference
and owned-output contracts remain. No test of a particular capacity value is
added. The complete 21-by-20 logical work CSV matches the actual control byte
for byte; arbitrary allocation/capacity bytes are outside that ledger.

## Complete unchanged comparison

Both campaigns retain every 42-condition group/cell, serial ABBA/BAAB,
48 samples/eight warmups per process/cell and 96 observations per
condition/group/cell. Capture, mutation, fresh leaf/all-root layout and fresh
complete owned export remain inside the timer; all vectors/digests agree.

| Selection/result | Observed | Predeclared requirement |
| --- | --- | --- |
| Edge complete p50 improvements | 6/12 | At least 8/12 |
| Small-edge complete p50 improvements | 2/4 | At least 3/4 |
| Candidate/ImGui conditions at most 2x | 30/42 | At least 30/42, all medium/large |

The first two conditions fail. No additional confirmation is needed to reject
an already ineligible treatment. Whole-operation p50 has 18 improvements,
22 regressions and two ties, with reductions -3.91% to +5.88%. P95 has 21
improvements and 21 regressions, -116.54% to +20.58%; maximum has 20/21/1,
-452.79% to +89.18%. Every favorable and adverse cell remains in the
[complete comparisons](2026-11-07-build-pending-reserve/comparisons.csv).
The independent ImGui result does not override the failed Rust-control
selection, and is not compared with another campaign to infer a speedup.

The [archive](2026-11-07-build-pending-reserve/evidence.tar.xz) preserves
426 inputs, 16128 raw observations and 336 full vectors, exact source pairs,
ordinary/work/build receipts, scripts and the prior source/location review.
All archive bytes round-trip against its hash manifest. The
[independent evidence review](2026-11-07-build-pending-reserve/independent-evidence-review.md)
recomputes the statistics and work/source bindings and supports rejection.
The [decision](2026-11-07-build-pending-reserve/decision.json) records exact
restoration before the next independently scoped placement-borrow experiment.
This rejection is about this initial-capacity tradeoff, without a claim that
the observed growth was fictitious or that all allocation work is unavoidable.
