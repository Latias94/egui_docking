# Rejected experiment: one child extent slab

Reject the two-file production candidate and restore both sources byte for byte
to `e3c5171`. The experiment replaces the three fresh child-minimum, maximum and
actual-extent Vecs with one fresh `3E` f64 allocation and borrowed segments. It
preserves the three public slices, every scalar and the original solver. The
[source review](2026-11-07-extent-slab/review-final.md) finds no semantic blocker;
correctness alone does not establish a performance benefit.

Ordinary counter-enabled qualification passes all 12 gates: 428 focused tests,
four work regressions, 2,069 production core, 2,245 workspace, four public
integrations and 292/335 host tests, plus fmt, correctness/suspicious Clippy,
strict docs, doctests and dependency checks. The same three opt-in probes remain
skipped. All 21 work rows and 20 columns match the control exactly, including six
successful root bodies and 2,926 rows. These counters do not measure allocations.

The frozen executable is SHA256
`22a1457de155c62b66c445c2a23b549b149e59172ed830f279333d409532124e`.
Against the actual row-capacity control and pinned ImGui, each comparison uses
both serial ABBA/BAAB groups, 48 samples/eight warmups per process/cell and 96
observations per condition/group/cell. Every complete vector and digest agrees.
Request capture, checked mutation, fresh all-root leaf input/layout and fresh
complete owned export remain in the measured interval.

Against `e3c5171`, complete-operation p50 has one improvement, 40 regressions
and one tie: the reduction range is -12.63% to +1.44%. All large group/cells
regress, by 3.01–9.60%. The p95 has 33 regressions and one tie, spanning -41.41%
to +35.05%; max has 22 regressions and one tie, spanning -515.25% to +89.33%.
The [complete table](2026-11-07-extent-slab/comparisons.csv) retains every result.
Fewer backing owners did not produce a measured win. Additional range operations,
generated code and memory layout are possible explanations; this experiment
provides no exclusive CPU or allocator attribution.

| Total panes | Candidate / ImGui complete p50, both groups | At most 2x |
| --- | --- | --- |
| 16 | 2.73–4.87x | 0/14 |
| 128 | 1.63–2.06x | 11/14 |
| 1024 | 1.08–1.49x | 14/14 |

The rejected candidate reaches 25/42 cells. The retained control remains 28/42;
this experiment establishes neither target completion nor an architectural
lower bound. Continue child-row composition, compact continuation and binary
input storage as separate candidates, rebased onto the retained three-Vec source.

The [archive](2026-11-07-extent-slab/evidence.tar.xz) contains 431 inputs plus
its manifest, 16,128 raw observations, 336 complete vectors, exact control and
candidate source pairs, ordinary checks/work and source reviews. All archive
members round-trip against their original bytes. The
[decision](2026-11-07-extent-slab/decision.json) binds the measured runtime patch
and verifies the exact two-source restoration. No rejected production code is
retained.
