# Reject borrowed projection topology after screening

Borrowing the current topology through synchronous projection removes one
temporary Arc retain/release per root, but fails the predeclared complete and
small-layout thresholds. The candidate is rejected and its only modified file,
`layout.rs`, is restored exactly to `c7508ee`. All 324 formal inputs match the
retained control; runtime `0e441c8` and its 30/42 target inventory remain.

The [independent continuation challenge](2026-11-07-borrowed-topology/source-review.md)
found this concrete owner deletion after the preceding seven experiments.
The source-matched diagnostic executable retains three topology Arc increments
and a temporary decrement: a synchronous local owner, followed by one durable
owner for each result map. These are reference-count operations on one
existing allocation, not three allocations or an exclusive time fraction.
The review asks for this bounded screen rather than approving a stop based
on the number of earlier rejections.

The private helper takes `&Arc<RootStructure>` instead of an owned Arc.
The direct public entry borrows the workspace's completed topology; delayed
`RootMeasurement` continues to own its Arc and measurement values, borrowing
that Arc only for projection. The raw adapter borrows its local compiled Arc.
Both result maps still acquire independent owners and all public output
remains fully owned. The
[implementation review](2026-11-07-borrowed-topology/implementation-review.md)
finds no source blocker: callbacks, override validation, arithmetic, raw DAG
behavior, late errors, delay and old-output lifetimes retain their contracts.
No new lifetime-bearing public API, cache, output representation or mirror
test is added.

Three normal-counter gates pass: formatting, 432 targeted tests and four
focused regressions. Ordinary work matches the 21-by-20 control CSV byte for
byte, and every complete vector/digest agrees. These are early-screen checks;
the remaining nine ordinary gates are not claimed and are required only for
a promising candidate before retention.

The actual retained Rust executable is
`3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`;
the candidate is
`73188fa44e168aaec6f591f029086b04dbc414ab0c4b79fcc21d44f4b0369d49`.
Both independent comparisons keep the original complete renderer-free timer,
42 group/cells, serial ABBA/BAAB, 48 samples/eight warmups per process/cell
and 96 observations per condition/group/cell.

| Selection/result | Observed | Predeclared requirement |
| --- | --- | --- |
| Complete p50 improvements against Rust control | 26/42 | At least 28/42 |
| Small layout p50 improvements | 3/14 | At least 8/14 |
| Candidate/ImGui conditions at most 2x | 30/42 | At least 30/42 and all medium/large |

Both Rust-control thresholds fail. Complete p50 has 26 improvements, sixteen
regressions and no ties, reductions -12.77% to +7.44%. P95 has 23/17/2,
-65.34% to +10.66%; maximum has 19/23/0, -1292.10% to +67.95%.
The [full table](2026-11-07-borrowed-topology/comparisons.csv) preserves all
observations. No confirmation replaces this failed initial screen.

The [archive](2026-11-07-borrowed-topology/evidence.tar.xz) preserves 418
inputs, 16128 raw observations and 336 complete vectors, exact source pairs,
predeclared rules, scripts, work/build/three-gate receipts and design and
implementation evidence. Every member passes a byte-for-byte manifest round
trip. The [decision](2026-11-07-borrowed-topology/decision.json) records rejection
and restoration. The
[independent evidence review](2026-11-07-borrowed-topology/independent-evidence-review.md)
reconstructs every phase statistic from raw observations and verifies vectors,
hashes, the screening boundary and restoration. This result rejects this private borrowed-topology treatment,
without establishing that Arc operations are free or assigning a compiler
cause to its complete-operation tradeoffs.
