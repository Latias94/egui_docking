# Reuse an exactly unchanged checked assigned total

Retain this bounded change with mixed performance. After the first checked
assigned-size reduction, exact equality to the finite target prevents both
grow and shrink. No intervening write, callback or output changes the size
slice, so repeating the same reduction in the same order returns the same
finite bits. Reuse that already checked scalar only on this equality path.
Every unequal path retains the original second checked sum after possible
writes, with the same diagnostic. Final tolerance, overflow and unallocated
construction remain byte-identical; no equality early return is introduced.

The one runtime hunk is +5/-1. There is one numeric executor and no new cache,
parameter, solver, arity tag, inline hint, axis specialization, fused transition
or dependency. The ordinary input checks, numerical/error order, rich packed
outputs, central/raw semantics, fresh leaf inputs and complete owned export
remain unchanged. The [source review](2026-11-07-exact-assigned-total/combined-source-review.md)
checks the exact-equality and signed-zero argument.

All twelve ordinary counter-enabled gates pass, including independent bitwise
numeric and full-layout oracles, real deep/raw/late-error behavior, staged
candidates, MRU and old outputs. The complete 21-by-20 logical-work CSV equals
packed control byte for byte. Both paired comparisons preserve every complete
vector and digest. Serial ABBA/BAAB groups retain 48 samples/eight warmups per
process/cell and 96 observations per condition/group/cell, under the original
request + checked mutation + fresh all-root layout + fresh owned export timer.

Candidate SHA256: `ccbc1be2fe9d0c14b63e8fbed845fda6332148786485c539b9166d6efdcca5f3`.
Actual control is `eb94cb2`, binary
`21888a3b2905da0edce33f68d4a7e3dbf68ba0e45d65f1d72df5d4d03f110435`.
Complete medians have 26 improvements, 13 regressions and three ties, ranging
from -1.90% to +8.32%. By panes, inventories are small 9/2/3, medium 11/3/0
and large 6/8/0. Large changes range from -1.30% to +1.79%; this is not a
universal improvement. P95 splits 21/21, range -25.57% to +20.83%; max has
17 improvements and 25 regressions, range -373.16% to +88.10%. Every negative
cell remains in the [full table](2026-11-07-exact-assigned-total/comparisons.csv)
and raw archive. No exclusive machine-code, allocator or phase-share cause
is inferred from the source deletion.

| Total panes | Complete p50 candidate / ImGui, both groups | At most 2x |
| --- | --- | --- |
| 16 | 2.0646–3.6222x | 0/14 |
| 128 | 1.2486–1.5697x | 14/14 |
| 1024 | 0.8714–1.1644x | 14/14 |

The target remains 28/42 and all small cells miss. Against ImGui, 40 p50,
38 p95 and 34 max cells trail. The two favorable large four-view edge medians
do not satisfy the complete target. Rich output and staged ownership contract
differences remain explicit, without deriving a latency lower bound.

The [archive](2026-11-07-exact-assigned-total/evidence.tar.xz) preserves 448
inputs plus manifest, 16,128 raw observations, 336 complete vectors, exact
source pairs/review, ordinary/work gates and source/build/comparison receipts.
The [decision](2026-11-07-exact-assigned-total/decision.json) retains only this
qualified hunk. Subsequent candidates must replay their own exact patch on
this actual retained parent and compare to this frozen binary; copying their
older complete layout file would erase this retained change. Continue the
axis boundary, persistent directory and binary measurement gather separately.

The [independent evidence review](2026-11-07-exact-assigned-total/evidence-review.md)
reproduces the exact retained one-hunk source, all qualification/work/vector
records and mixed statistics. Its appendixed parent preflight confirms that
remaining patches can replay without erasing this hunk; it does not transfer
qualification or predict a subsequent benefit.
