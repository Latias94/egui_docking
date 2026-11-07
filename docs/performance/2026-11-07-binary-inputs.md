# Local binary inputs to the unchanged axis solver

Keep the one-file input-storage change. Exactly two actual children select a
local pair of AxisConstraints; exactly two children and graph weights select a
local f64 weight pair. Other shapes clear/refill the original scratch Vecs.
Selected overrides retain their entire existing f64 slice and precedence.
Every branch joins the original checked axis solver, without new arithmetic,
padding, normalization or truncated cardinalities.

The [original source review](2026-11-07-binary-inputs/original-source-review.md),
[three-vector review](2026-11-07-binary-inputs/three-vector-review.md) and
[actual retained-parent review](2026-11-07-binary-inputs/retained-parent-review.md)
bind the one-file delta to the compact-frame control. The rejected slab and
child-row composition remain absent. No test or numeric solver is changed.
Mixed binary/nonbinary, full/minimum, override, raw diagnostic, owned-output and
deep traversal cases remain in the existing independent-reference coverage.

All 12 ordinary counter-enabled gates pass: 429 focused tests, four work
regressions, 2,070 production core, 2,246 workspace, four public integrations
and 292/335 host tests, plus fmt, correctness/suspicious Clippy, strict docs,
doctests and dependency checks. The same three opt-in probes remain skipped.
The entire 21-row, 20-column work CSV matches the compact-frame control,
including six successful root bodies and 2,926 rows.

Frozen candidate SHA256:
`4a41d0c57fcc97a256638919163c6d1d70f09473c3c6a56b8855ead5edfe4cf8`.
Actual control is `dc689a6`, binary
`7a707c2fe841cc5a7fb57027c36a1706099b087dfe1e4302499630af2e499001`.
Both complete comparisons, including pinned ImGui, use serial ABBA/BAAB groups,
48 samples/eight warmups per process/cell and 96 observations per condition/
group/cell. All complete vectors and digests agree. Request capture, checked
mutation, fresh all-root leaf inputs/layout and fresh complete owned export
remain inside the complete-operation timer.

Against the compact-frame control, 40/42 complete p50 cells improve and two
regress, with a reduction range of -1.26% to +8.52%. All fourteen 1024-pane
group/cells improve, by 1.16–5.55%. The two negative medians are group 1,
16-pane single-view edge (-0.028%), and group 2, 128-pane single-view resize
(-1.26%). P95 has six regressions, range -12.50% to +25.13%; max has 17
regressions, range -185.08% to +82.84%. The
[complete table](2026-11-07-binary-inputs/comparisons.csv) retains every result.

| Total panes | Complete p50 / ImGui, both groups | At most 2x |
| --- | --- | --- |
| 16 | 2.12–4.20x | 0/14 |
| 128 | 1.37–1.80x | 14/14 |
| 1024 | 0.92–1.28x | 14/14 |

The total remains 28/42; every small cell still exceeds 2x. Large four-view
edges are faster than ImGui in both groups in this campaign; the other 40 p50,
41 p95 and 33 max cells trail it. The goal remains active.

Scratch was already reused across a root, so this does not save two backing
allocations per split. All-binary roots can avoid filling those two fallback
buffers; mixed roots retain their costs. Local array storage, branches, code
size and compiler specialization can also change. No actual allocator share,
allocation-byte saving or exact stack size is established by these timings or
logical counters. Continue unified values and the narrower committed weight
check, then concrete command ownership work.

The [archive](2026-11-07-binary-inputs/evidence.tar.xz) retains 431 inputs plus
its manifest, 16,128 raw observations, 336 complete vectors, exact source pairs,
ordinary checks/work and source reviews. Every member round-trips against its
original bytes. The [decision](2026-11-07-binary-inputs/decision.json) preserves
the retention boundary and all complete statistics. New runtime heads require
fresh remote CI.
