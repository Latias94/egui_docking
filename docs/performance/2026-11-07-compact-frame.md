# Compact projection continuations on retained gather-based layout

Keep the compact continuation for the measured large-workload improvement. Each
frame now carries its row, next child, cursor, original splitter thickness and
pending-child flag. The graph-owned row and already recorded NodeResult supply
the structural facts and parent bounds. On return from a child, the continuation
reads its original solved extent from the existing result. Child construction,
complete descendant traversal and the following parent gap retain their order.

The [original source review](2026-11-07-compact-frame/original-source-review.md),
[three-Vec review](2026-11-07-compact-frame/three-vector-review.md) and
[actual retained-parent review](2026-11-07-compact-frame/retained-parent-review.md)
bind the exact source composition. Neither rejected slab nor direct child-row
composition is present. One private row reader and the original 55-line
descendant-before-parent-gap regression accompany the continuation change.
The original numeric solver, validations, leaf input and complete output stay.

All 12 ordinary counter-enabled gates pass: 429 focused tests, four work
regressions, 2,070 production core, 2,246 workspace, four public integrations and
292/335 host tests, plus fmt, correctness/suspicious Clippy, strict docs,
doctests and dependency checks. The same three opt-in probes remain skipped.
All 21 work rows and 20 columns match `e3c5171` exactly, including six successful
root bodies and 2,926 rows. Logical-work counts do not measure frame size,
allocation calls, allocator bytes or exclusive allocator time.

Frozen candidate SHA256:
`7a707c2fe841cc5a7fb57027c36a1706099b087dfe1e4302499630af2e499001`.
Both complete comparisons against the actual row-capacity control and pinned
ImGui use serial ABBA/BAAB groups, 48 samples/eight warmups per process/cell and
96 observations per condition/group/cell. Every full vector and digest agrees.
Request capture, checked mutation, fresh all-root leaf inputs/layout and fresh
complete owned export remain timed.

Against `e3c5171`, 35/42 complete p50 group/cells improve, five regress and two
tie. The reduction range is -4.41% to +6.57%. Every 1024-pane group/cell improves,
by 2.05–6.49%. Small results remain mixed: group 1 single-view resize and edge
regress, as do group 2 four-view Center, edge and cross-view moves. P95 has 11
regressions, range -26.83% to +24.95%; max has 25 regressions, range -299.04% to
+84.58%. The [complete table](2026-11-07-compact-frame/comparisons.csv) retains
all adverse observations. Retention is a bounded tradeoff, not uniform speedup.

| Total panes | Complete p50 / ImGui, both groups | At most 2x |
| --- | --- | --- |
| 16 | 2.47–4.50x | 0/14 |
| 128 | 1.47–1.91x | 14/14 |
| 1024 | 0.97–1.32x | 14/14 |

The total remains 28/42; every small group/cell still exceeds 2x. Large four-view
edges are faster than ImGui in both groups in this campaign; the other 40 p50
cells trail it. ImGui also leads 41 p95 and 35 max cells. The goal stays active.

The frame moves fewer source facts but adds topology/NodeResult/extent reads.
Compiler elimination, frame moves and bounds checks can offset the saving, and
default Rust enum layout supplies no fixed byte-size promise. No exclusive cause
is established. Continue independent binary-input and ownership/validation
experiments rather than inferring a lower bound from the remaining ratio.

The [archive](2026-11-07-compact-frame/evidence.tar.xz) preserves 435 inputs plus
its manifest, 16,128 raw observations, 336 complete vectors, all checks/work,
exact control/candidate source pairs and the independent source reviews. Every
member round-trips against the original bytes. The
[decision](2026-11-07-compact-frame/decision.json) preserves the complete result
counts and retention boundary. Fresh remote CI is required for this runtime head.

The [independent evidence review](2026-11-07-compact-frame/evidence-review.md)
binds immutable `dc689a6` to e3 and independently recomputes every complete
statistic and vector. It supports the bounded retention decision, discloses
the adverse small/tail cells, and confirms the overall target is incomplete.
