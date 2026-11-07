# Materialized parent-edge projection results

Retain the complete parent-edge result representation. Keep the fresh node
buffer and separate split outcomes; replace the three extent arrays and gap
array with one edge-record buffer containing materialized minimum, maximum,
actual extent and optional following gap. The public borrowed split view now
exposes `children: &[ChildProjection]`; the previous parallel slice fields are
removed. Consumers migrate directly without compatibility arrays.

The numerical solver and its full checked entry remain byte-identical. Binary
sizes use a local array; other arities reuse root-local scratch. Every solved
extent is copied into its edge record immediately, inside projection. Minima
and maxima are stored at their original phase, and each gap is constructed only
after its descendant returns. Parent-edge ownership preserves raw shared-DAG
allocations and last-write behavior. Dead topology gap ranges/counts are removed;
split outcome ordinals and actual error-count arithmetic remain.

The [independent source review](2026-11-07-packed-results/independent-source-review.md)
checks the retained fully validating parent and closes a metadata-only sorted
path-digest correction. No rejected axis specialization, slab or child-row
iterator is included. Both cost probes are byte-identical. The source delta
contains six production files (+138/-105), an oracle capture adapter and four
mechanical test migrations, eleven paths total. The command ownership change
already belongs to the actual control, `a2365aa`.

All twelve ordinary counter-enabled gates pass: 432 focused, four work
regressions, 2,073 production core, four public integrations, 2,249 workspace,
292/335 host tests, formatting, correctness/suspicious Clippy, strict docs,
doctests and dependency boundaries. The same three opt-in probes remain skipped.
The complete 21-row, 20-column work CSV equals the command control, including
six successful root bodies and 2,926 rows. These are logical-work counts.

Frozen candidate SHA256:
`21888a3b2905da0edce33f68d4a7e3dbf68ba0e45d65f1d72df5d4d03f110435`.
Control binary SHA256:
`d188a009676e1a0a523cf93c665f7f95be1df559d3b76522da0fa4459da261b5`.
Both control and pinned ImGui comparisons use serial ABBA/BAAB groups,
48 samples/eight warmups per process/cell and 96 observations per condition/
group/cell. Every full vector and digest agrees. Request capture, checked
mutation, fresh all-root leaf inputs/layout and fresh complete owned export
remain inside the complete interval.

Against the command control, 33/42 complete p50 group/cells improve, five
regress and four tie. Reductions range from -1.75% to +10.80%. Small four-view
center, cross-view and bounds-only resize improve in both groups. P95 improves
in 29, regresses in twelve and ties in one, range -23.28% to +25.89%. Max splits
21 improvements/21 regressions, range -1,920.32% to +82.88%. Every tail and
aggregate comparison remains in the [full table](2026-11-07-packed-results/comparisons.csv);
individual observations remain in archived raw CSVs.

The five negative p50 cells are group 1 medium four-view center (-0.364%),
large single-view resize (-0.222%) and center (-1.394%); group 2 small
single-view edge (-1.276%) and large four-view cross-view (-1.746%).

| Total panes | Complete p50 / ImGui, both groups | At most 2x |
| --- | --- | --- |
| 16 | 2.07–3.87x | 0/14 |
| 128 | 1.22–1.65x | 14/14 |
| 1024 | 0.90–1.20x | 14/14 |

The target remains 28/42. Two large four-view edge medians beat ImGui; forty
p50, thirty-eight p95 and twenty-five max cells trail it. All small cells
still exceed 2x. Continue the independently reviewed continuation stack and
control-flow candidates, with their own complete comparisons.

Negative costs remain visible: wider records reserve optional gap storage for
final children; sizes scratch is initialized and E actual values are copied;
general arity can allocate another temporary owner; scalar scans traverse wider
records. The scene compiler gathers a continuous resize Arc once per rendered
split, with possible staging allocation. This primary binary kernel does not
measure that scene path or establish general-arity throughput, allocation-byte
savings, allocator shares or memory bounds. All constraints and old outputs
retain complete materialized values.

The [archive](2026-11-07-packed-results/evidence.tar.xz) preserves 483 inputs
plus its manifest, 16,128 raw observations, 336 complete vectors, exact source
pairs, ordinary qualification/work and reviewed package history. Every member
round-trips against the original bytes. The [decision](2026-11-07-packed-results/decision.json)
binds the exact retention boundary. Fresh remote CI is still required; the
earlier unified head currently has a separate physical drag-back failure under
investigation, and its prior binary-input head was green.

The [independent evidence review](2026-11-07-packed-results/independent-evidence-review.md)
recomputes all observations, full vectors and statistics, verifies the eleven
source-pair and corrected metadata bindings, and confirms the 33/5/4 p50
result. It finds no evidence blocker and retains all representation costs,
including the scene path not measured by the primary kernel.
