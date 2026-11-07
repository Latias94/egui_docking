# Rejected experiment: avoid repeated committed axis-input checks

Reject this exact two-file slice and restore both sources byte for byte to
`07f566a`. The private projection chain specialized committed graph calls to
validate only the runtime extent; checked raw calls and selected overrides
retained the entire original validator. Both branches called the same original
numerical executor. The redundant private wrapper was removed. No arithmetic,
fresh leaf inputs, complete output or override validation was omitted.

The [boundary review](2026-11-07-axis-inputs/boundary-design-review.md),
[original review](2026-11-07-axis-inputs/original-source-review.md) and
[retained-parent review](2026-11-07-axis-inputs/retained-parent-source-review.md)
found no source blocker. All twelve ordinary counter-enabled gates pass:
433 focused tests, four work regressions, 2,074 production core, four public
integrations, 2,250 workspace and 292/335 host tests, plus fmt, correctness/
suspicious Clippy, strict docs, doctests and dependency checks. The same three
opt-in probes remain skipped. The complete 21-row, 20-column work CSV is
byte-identical to the unified-value control: six successful root bodies and
2,926 rows. These are logical-work counts, not allocator attribution.

Frozen candidate SHA256:
`b361307359e199615dfb5d491187eb485f35a4652a03bd66732992c99b061c01`.
Actual control is `07f566a`, binary
`270bbd69fd9856dddb8ea22ba0a9ed7edfa74a0b3e09a0f3101dac89f9a9300f`.
Both control and pinned ImGui comparisons use serial ABBA/BAAB groups,
48 samples/eight warmups per process/cell, and 96 observations per condition/
group/cell. All complete vectors and digests agree. The timer includes request
capture, checked mutation, fresh all-root leaf inputs/layout and fresh complete
owned export.

Against the retained control, one complete p50 improves and 41 regress; there
are no ties. The reduction range is -24.47% to +1.54%. The sole improvement is
group 1, 16 panes, four views, cross-view move: 2,666 to 2,625 ns (+1.538%).
All fourteen large group/cells regress by 11.75–24.47%. P95 has 38 regressions,
range -41.17% to +41.91%; max has 28 regressions, range -636.33% to +88.64%.
The [complete table](2026-11-07-axis-inputs/comparisons.csv) preserves every
observation. The experiment establishes no exclusive compiler, locality,
branching or allocator cause for the regression.

| Total panes | Complete p50 candidate / ImGui, both groups | At most 2x |
| --- | --- | --- |
| 16 | 2.53–4.37x | 0/14 |
| 128 | 1.51–2.01x | 12/14 |
| 1024 | 1.05–1.56x | 14/14 |

The rejected candidate reaches 26/42, while the retained control remains
28/42. All 42 candidate p50 and p95 cells trail ImGui; 31 max cells also trail.
Removing source-level checks did not yield a complete-operation improvement.
This result rejects this implementation; it does not prove that validation is
free or that all other ways to simplify the boundary would regress.

The [archive](2026-11-07-axis-inputs/evidence.tar.xz) preserves 433 inputs plus
its manifest, 16,128 raw observations, 336 complete vectors, exact source pairs,
all qualification/work, and source reviews. Every member round-trips against
its original bytes. The [decision](2026-11-07-axis-inputs/decision.json) binds
the exact frozen patch and verifies both restored source identities. The
candidate's two additional validation witnesses remain in the rejected
experiment archive. No rejected runtime or test overlay is retained.

Continue command ownership and the independent complete parent-edge output
experiment on the retained, fully checked parent. The target remains active;
this is not an architectural lower bound or a stopping decision.
