# Reject release prefix bookkeeping removal after screening

Removing the private projection prefix bookkeeping from release builds does
not satisfy the predeclared complete-operation thresholds. The candidate is
rejected and `layout.rs` is restored exactly to `c7508ee`, including the
original release guard. All 324 formal inputs match the retained control;
runtime `0e441c8` and the 30/42 target inventory remain unchanged.

Two independent current-source reviews identified this as a concrete small
experiment. Production and raw graph builders number first visits in DFS
order, and no public input can supply the manually constructed private row
gap guarded here. Static disassembly of the source-matched diagnostic still
contains the prefix load, comparison, panic branch, conditional increment
and store per Enter. This establishes surviving work, not its time share.
The [source review](2026-11-07-debug-prefix/source-review.md) records that
evidence and explains why the larger traversal alternatives carry additional
construction and ownership costs.

The candidate places the prefix variable, check and increment together under
`cfg(debug_assertions)`; normal indexing bounds, raw graph revisits, geometry,
error order and debug defenses remain. Its
[explicit scope refinement](2026-11-07-debug-prefix/scope-refinement.json)
permits changing release behavior for an illegal, manually constructed
private structure. It does not claim every internal panic remains identical.
This internal defense change is also discarded with the rejected candidate.

Three normal-counter gates pass: formatting, 432 targeted tests and four
focused regressions. The 21-cell, 20-column ordinary work CSV remains
byte-identical to the control, and all complete output vectors and digests
agree. This is an early performance screen, not twelve-gate qualification;
the remaining nine gates and an additional confirmation are required only
for a promising candidate before retention.

The actual Rust control executable is
`3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`;
the candidate is
`a2d99eb603d2e1612020246473d2442517c093534ffb348d3b79718b0ab7be84`.
Both comparisons keep the original renderer-free complete-operation timer,
42 group/cells, serial ABBA/BAAB order, 48 samples and eight warmups per
process/cell, and 96 observations per condition/group/cell.

| Selection/result | Observed | Predeclared requirement |
| --- | --- | --- |
| Complete-operation p50 improvements against Rust control | 14/42 | At least 28/42 |
| Small layout p50 improvements | 3/14 | At least 8/14 |
| Candidate/ImGui conditions at most 2x | 30/42 | At least 30/42 and every medium/large condition |

The first two requirements fail. Complete p50 has 14 improvements, 25
regressions and three ties, reductions -15.52% to +5.61%. P95 has 19/23/0,
-35.77% to +34.49%; maximum has 17/24/1, -1197.50% to +80.62%.
The [complete table](2026-11-07-debug-prefix/comparisons.csv) retains every
negative observation. No additional campaign replaces these results.

The [archive](2026-11-07-debug-prefix/evidence.tar.xz) contains 418 inputs,
16128 raw observations and 336 complete output vectors, exact control and
candidate sources, predeclared thresholds, build/work/three-gate receipts
and the original scripts. Every archive member passes a byte-for-byte
manifest round trip. The [decision](2026-11-07-debug-prefix/decision.json)
records rejection and restoration. The
[independent evidence review](2026-11-07-debug-prefix/independent-evidence-review.md)
recomputes all comparisons and archive hashes and accepts rejection; the
completed checksum list also includes the decision. This rejects the measured bookkeeping
tradeoff; it does not establish that defensive checks are free or that the
remaining performance difference cannot be reduced.
