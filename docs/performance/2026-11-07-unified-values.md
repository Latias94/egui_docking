# One fresh node-value owner from measurement through projection

Keep one fresh Vec of NodeResult values. The original postorder computes
constraints into that buffer; projection moves it into NodeMap and writes only
the actual rectangles. Remove the separate N-element measurement owner and
later constraints transfer. Production retains only the matching successfully
read central Tabs row; a local preorder frontier chooses its containing child.
No central ancestor Vec, reversal, binary search or persistent metadata is added.

Retain the original slice-based composition and child gather Vecs. Checked raw
compilation and the entire independent reference remain byte-identical to the
retained parent. After successful raw compilation, all true membership rows
are adapted to the same projection executor, preserving shared DAG parents,
repeated Enter and last-write behavior. Each Enter enforces the old release
no-gap prefix rule; final completeness retains its debug check. The
[retained-parent source review](2026-11-07-unified-values/retained-parent-source-review.md)
binds the exact composition and the two added semantic regressions.

The archived first draft had only a debug prefix guard and was rejected before
application. Its [source review](2026-11-07-unified-values/first-rejected-source-review.md)
and the subsequent
[scalar-parent review](2026-11-07-unified-values/scalar-parent-source-review.md)
remain diagnostic history. They are not executed candidates. The measured
version restores release rejection and contains neither rejected slab nor
direct child-row composition.

All 12 ordinary counter-enabled gates pass: 431 focused tests, four work
regressions, 2,072 production core, 2,248 workspace, four public integrations
and 292/335 host tests, plus fmt, correctness/suspicious Clippy, strict docs,
doctests and dependency checks. The same three opt-in probes remain skipped.
The complete 21-row, 20-column work CSV equals the binary-input control, including
six successful root bodies and 2,926 rows. These are logical-work counts.

Frozen candidate SHA256:
`270bbd69fd9856dddb8ea22ba0a9ed7edfa74a0b3e09a0f3101dac89f9a9300f`.
Actual control is `59a4980`, binary
`4a41d0c57fcc97a256638919163c6d1d70f09473c3c6a56b8855ead5edfe4cf8`.
Both complete control and pinned ImGui comparisons use serial ABBA/BAAB groups,
48 samples/eight warmups per process/cell and 96 observations per condition/
group/cell. Every full vector and digest agrees. Request capture, checked mutation,
fresh all-root leaf inputs/layout and fresh complete owned export remain timed.

Against the control, 39/42 complete p50 group/cells improve, two regress and one
ties; the reduction range is -2.88% to +10.28%. The negative medians are group 1
large four-view cross-view move (-0.146%) and group 2 small single-view resize
(-2.881%). P95 has ten regressions and one tie, range -19.27% to +22.83%;
max has sixteen regressions and one tie, range -339.62% to +88.38%. The
[complete table](2026-11-07-unified-values/comparisons.csv) retains every result.

| Total panes | Complete p50 / ImGui, both groups | At most 2x |
| --- | --- | --- |
| 16 | 2.06–3.94x | 0/14 |
| 128 | 1.27–1.73x | 14/14 |
| 1024 | 0.90–1.20x | 14/14 |

The total remains 28/42; every small group/cell still exceeds 2x. Two large
four-view edge medians beat ImGui, while 40 p50, 38 p95 and 31 max cells trail
it. This is another retained improvement, not completion or a lower bound.

The tradeoff includes initializing rectangle placeholders before they are used.
A minimum-only measurement later discarded after clipping now retains otherwise
unused rectangle storage. Gather copies remain, record stride changes, and the
release prefix plus central frontier perform actual work. Timings establish no
exclusive allocator cause, allocation-byte count or peak residency. Continue
committed weight checks and command ownership, then assess remaining structural
output/storage options against the new complete measurements.

The [archive](2026-11-07-unified-values/evidence.tar.xz) preserves 439 inputs plus
its manifest, 16,128 raw observations, 336 complete vectors, exact source pairs,
all checks/work, complete parent composition and independent source reviews.
Every member round-trips against its original bytes. The
[decision](2026-11-07-unified-values/decision.json) binds the exact candidate and
retention boundary. Fresh remote CI is required for new runtime heads.
