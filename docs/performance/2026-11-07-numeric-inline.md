# Rejected experiment: ordinary inline on the numerical executor

Reject this exact one-line ordinary `#[inline]` hint before the existing
`solve_validated_axis_into`. Its entire arithmetic/validation implementation,
packed output writes, standard-Vec continuation and complete probe remain
unchanged. No fused transition, axis specialization, exact-total reuse or
persistent storage is included. The source change offers no source-level
allocation, validation or arithmetic deletion.

All twelve ordinary counter-enabled gates pass: focused semantics, production
core, workspace, public integrations, host default/serde, formatting, lint,
strict docs, doctests and dependency boundary. The 21-by-20 work CSV equals
packed control byte for byte. Both complete comparisons preserve every vector
and digest. Serial ABBA/BAAB groups retain 48 samples/eight warmups per
process/cell and 96 observations per condition/group/cell; capture, checked
mutation, all-root fresh leaf/layout and complete fresh owned export remain
inside the original timer.

Candidate SHA256: `56fb894c43f61e3dd70bdc7c38451a607a7c221592a813c5482f6c18678e2b14`.
Control is `eb94cb2`, binary
`21888a3b2905da0edce33f68d4a7e3dbf68ba0e45d65f1d72df5d4d03f110435`.
Complete p50 has 18 improvements, 19 regressions and five ties, range
-3.91% to +8.57%. Eleven of fourteen large medians regress; their three
improvements are below 0.30%. Small medians are mixed across groups, including
single-view regressions in group one. P95 has 22 improvements, 17 regressions
and three ties, range -21.33% to +58.71%; max has 22 improvements and 20
regressions, range -749.30% to +85.92%. All cells and tails remain in the
[complete table](2026-11-07-numeric-inline/comparisons.csv).

| Total panes | Complete p50 candidate / ImGui, both groups | At most 2x |
| --- | --- | --- |
| 16 | 2.2000–3.6852x | 0/14 |
| 128 | 1.2198–1.6667x | 14/14 |
| 1024 | 0.8682–1.1558x | 14/14 |

The result remains 28/42 and every small cell misses. Against ImGui, 40 p50,
39 p95 and 22 max cells trail. The hint has not earned a stable retained gain;
favorable tails and individual medians are retained in the evidence rather
than being used to select a score. These results do not establish whether
inlining occurred, its dynamic call share or an exclusive compiler cause.

The [archive](2026-11-07-numeric-inline/evidence.tar.xz) preserves 448 inputs
plus manifest, 16,128 observations, 336 complete vectors, source pairs,
source review, ordinary/work qualification and exact build/comparison receipts.
The [decision](2026-11-07-numeric-inline/decision.json) verifies that reversing
only the frozen single-line delta restores the entire layout to `eb94cb2`.
Continue the exact unchanged-assigned-total reuse, axis compiler-boundary,
persistent-directory and binary-measurement candidates. This rejection is not
a stopping decision.

The [independent evidence review](2026-11-07-numeric-inline/evidence-review.md)
reproduces every result and immutable source restoration. Small four-view
center and edge improve in both groups; the single-view directions vary.
Those favorable small cells remain explicit alongside the broad large-cell
regression and unchanged target count.
