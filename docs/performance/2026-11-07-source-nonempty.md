# Reject source-nonempty reuse under the resize protection rule

This treatment removes a supported redundant existence query but is rejected
under its predeclared resize protection. The retained runtime remains
`0e441c8`; all 324 formal source hashes are restored to actual control
`c7508ee`. No runtime or added test change survives.

After successfully removing an Item, the private writer returns whether its
source Tabs still contains an item. Its two immediate callers pass that fact
to root cleanup. The original root lookup and central rule remain first;
source-empty cases still execute the original subtree query and presentation
retirement. The close caller's intervening successor selection only changes
selected/MRU. No cached count, new owner, weakened reference or skipped
rollback is introduced. The old query exits at the first nonempty leaf; the
treatment does not claim to remove a complete root walk on every move.

Both [source review](2026-11-07-source-nonempty/source-review.md) and
[contract review](2026-11-07-source-nonempty/contract-review.md) find no source
blocker. All twelve ordinary gates pass: 432 targeted tests, four focused
work regressions, 2073 production core tests, 2249 workspace tests, four public
integration tests, 292/335 independent-host tests, ten doctests, formatting,
strict documentation, dependency checks and correctness/suspicious Clippy.
The complete 21-by-20 logical work CSV matches the actual control byte for
byte; that ledger does not count arbitrary traversal allocations.

## Complete-operation results

Frozen candidate:
`0ac7566065a4a7b23b88b2c7f0c690122d0ae6cd4e7227693f85f29b73e48317`.
The fixed workload/timer, all 42 group/cells, serial ABBA/BAAB ordering,
48 samples/eight warmups per process/cell and 96 observations per
condition/group/cell remain unchanged. Fresh full vectors and digests agree.

| Rust pairing | Complete p50 improve/regress/tie | Move p50 | Small move p50 | Resize p50 |
| --- | --- | --- | --- | --- |
| Initial full pairing | 22/19/1 | 21/9/0 | 10/0/0 | 1/10/1 |
| Additional full confirmation | 26/14/2 | 21/8/1 | 9/1/0 | 5/6/1 |

The predeclared move thresholds, at least 20/30 move improvements and 6/10
small move improvements, pass twice. The separate ImGui campaign has 30/42
conditions at most 2x, including all medium/large conditions. It does not
replace the comparison against the retained Rust control.

The same rule also rejects repeatable material resize regressions. The first
pairing triggers one additional full Rust pairing to resolve that issue;
neither pairing is discarded or replaced. Confirmation group one has all six
resize medians regress, while group two improves or ties in every resize
condition. Those group effects are preserved, not called a universal slowdown
or attributed exclusively to code generation. Nevertheless, 1024 panes/one
view resize changes by -3.20%, -2.99%, -2.18% and +0.30% across the four
group/pairings, with all four p95 comparisons regressing. The 128-pane/one-view
resize p95 also regresses four times. This does not support marking the
protection condition passed merely because small moves improve.

The [independent decision review](2026-11-07-source-nonempty/decision-review.md)
supports rejection without a fourth measurement. Keeping this mixed tradeoff
would require accepting the resize negatives as a product decision, rather
than claiming the declared rule passed. This continuation preserves the rule.

Initial complete p50 reductions span -14.22% to +23.61%, p95 -36.72% to
+64.00%, and maximum -1088.82% to +83.40%. Confirmation p50 spans -6.18% to
+19.26%, p95 -108.78% to +63.70%, and maximum -610.99% to +92.30%. All raw
tail spikes and favorable samples remain; neither median nor maximum is
presented as a guaranteed bound.

The [initial tables](2026-11-07-source-nonempty/comparisons.csv),
[confirmation tables](2026-11-07-source-nonempty/confirmation-comparisons.csv)
and [decision](2026-11-07-source-nonempty/decision.json) remain separate.
The [archive](2026-11-07-source-nonempty/evidence.tar.xz) preserves 623 input
files, 24192 raw observations and 504 complete vectors, all source/build/work
receipts, both ordinary-check attempts including the initial formatting
failure, the corrected formatted candidate and exact control. The original
formatting issue was fixed before all twelve successful gates and freezing.
Its manifest is verified by complete archive round trip.

This rejection concerns this private bool propagation and its emitted code.
It does not establish that eliminating existence queries is always harmful,
nor remove the new current-source evidence for independently scoped pending
stack, placement borrowing and equal-binary numerical experiments.
