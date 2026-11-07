# Reject the exact equal-binary desired-size shortcut after confirmation

This experiment is numerically correct but does not earn performance retention.
Its initially promising complete medians fail to persist in the full additional
Rust confirmation. Both modified files, including the added boundary witness,
are restored exactly to `c7508ee`. All 324 formal source hashes match the
retained runtime `0e441c8`; its formal target inventory remains 30/42.

The treatment recognizes exactly two equal actual weights without a central
child at the original desired-size stage. Full validation, minimum and maximum
aggregate checks, infeasible returns, clamps, assigned sums, grow/shrink and
outcomes remain in place. Validation proves positive finite equal weights and
a finite total: binary doubling is exact, so their normalized share is exactly
0.5. Multiplying the original extent by 0.5 preserves desired bits, including
signed zero and subnormal underflow. Central, unequal and nary cases retain
the original helper. There is no scalar-total ABI, approximate equality,
second-size subtraction, cached result or replacement correction algorithm.

The source-matched control still executes the generic desired reduction and
division, as documented in the [post-main diagnosis](2026-11-07-post-main-diagnostic.md).
This is a distinct work-deleting hypothesis from the previously rejected
initializer unrolling and validated-total propagation. It does not follow
that its branch and code-generation tradeoffs improve complete latency.

## Qualification and complete measurements

Candidate binary:
`fc17c426ae6c25d747a2231807d187866c30bedde35cdb32c9abd933978998a6`.
Actual Rust control is `c7508ee`, binary
`3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`.
All twelve ordinary counter-enabled gates pass, including 433 targeted,
2074 production core, 2250 workspace, four public integration, 292/335
independent-host and ten documentation tests, formatting, strict docs,
dependencies and correctness/suspicious Clippy. The complete 21-by-20 work
CSV remains byte-identical. Fresh complete vectors and digests agree.

One fixed semantic test contains 31 solver invocations against the unchanged
reference: finite doubling boundaries, signed zeros/subnormals, feasible MAX
extent, clamped grow/shrink, both infeasible outcomes, unequal/central/nary
fallback and thirteen conflicting-error cases. Successful sizes and outcomes
compare by bits; NaN-bearing errors compare variants, indexes and bits. The
existing randomized and complete-layout oracles remain unchanged. The test
is archived with the rejected candidate rather than retained alone.

The original 42-condition renderer-free complete-operation workload, including
capture, checked mutation, fresh leaf/all-root layout and fresh owned export,
remains unchanged. Every pairing uses serial ABBA/BAAB, 48 samples/eight
warmups per process/cell and 96 observations per condition/group/cell.

| Rust pairing | Complete p50 improve/regress/tie | Small layout p50 improvements | Complete p95 improve/regress/tie | Maximum improve/regress/tie |
| --- | --- | --- | --- | --- |
| Initial | 34/8/0 | 13/14 | 20/22/0 | 24/18/0 |
| Additional confirmation | 20/20/2 | 6/14 | 21/21/0 | 21/21/0 |

The predeclared rule requires at least 28 complete median improvements,
8/14 small layout improvements, at least 30 ImGui target cells and every
medium/large cell at most 2x, with repeated material tail losses blocking
retention. Initial numerical gates pass. The separate candidate/ImGui pairing
has 31/42 target cells; this is not a historical 30-to-31 runtime improvement.

An independent decision challenge identifies repeated initial p95/max losses
for 1024 panes/four views center and cross-view, motivating one full additional
Rust pairing. The small single-view edge p95 is also adverse twice. The
confirmation remains additive: no first-pair negative or comparator is
replaced, no favorable group is selected, and no fourth measurement is used
to seek acceptance. Confirmation fails both complete and small-layout gates.
Large four-view center maximum still regresses in both confirmation groups,
while several p95 comparisons change direction. This is sufficient to reject
the candidate without claiming universal tail degradation or a compiler cause.

Initial complete p50 reductions span -4.06% to +10.00%, p95 -21.05% to
+71.07%, and maximum -336.50% to +88.25%. Confirmation p50 spans -13.64%
to +6.87%, p95 -123.23% to +66.04%, and maximum -726.09% to +77.73%.
All negative samples remain in the [initial comparison](2026-11-07-equal-binary/comparisons.csv)
and [confirmation comparison](2026-11-07-equal-binary/confirmation-comparisons.csv).
These observed statistics are not guaranteed bounds or an exhaustive claim
about binary algorithms, language, ownership or achievable latency.

The [archive](2026-11-07-equal-binary/evidence.tar.xz) preserves 628 inputs,
24192 raw observations and 504 complete vectors, all normal/work/build/source
receipts, the candidate/reference boundary and the original confirmation
request. A full manifest round trip verifies archived bytes. The
[independent evidence audit](2026-11-07-equal-binary/independent-evidence-review.md)
and [final decision review](2026-11-07-equal-binary/final-decision-review.md)
support rejection and the limited scope of that conclusion. The
[decision](2026-11-07-equal-binary/decision.json) records rejection and exact
restoration. Independently located graph-build and placement-storage work
remain separate hypotheses on the unchanged retained runtime.
