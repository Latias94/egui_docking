# Rejected experiment: compose constraints directly from child rows

Reject the three-file candidate and restore every touched source byte for byte
to `e3c5171`. Production constraint composition reads copied Constraints through
standard cloneable exact-size child-row iterators, removing the temporary child
gather Vec. Delayed maximum completion adopts the same iterator. Raw diagnostic
gathers remain; the independent traversal changes only its two helper arguments.
Numeric reduction order, first errors, fresh inputs and complete output stay.

The [original source review](2026-11-07-child-rows/review-final.md) and
[three-Vec rebase review](2026-11-07-child-rows/rebase-review.md) found no source
blocker. This candidate contains none of the rejected slab. All 12 ordinary
gates pass: 428 focused, four work regressions, 2,069 production core, 2,245
workspace, four public integrations and 292/335 host tests, plus formatting,
correctness/suspicious Clippy, strict docs, doctests and dependency checks. The
same three opt-in probes remain skipped. The complete 21-row, 20-column work CSV
is byte-identical to the actual control, with six successful root bodies and
2,926 rows. It provides no allocator-event or allocation-byte attribution.

Frozen candidate SHA256:
`93b6c26d8b47b42bf34a0867a7766836f5881a49a75afdea4a415a284661d6df`.
Both comparisons against the row-capacity control and pinned ImGui use serial
ABBA/BAAB groups, 48 samples/eight warmups per process/cell, and 96 observations
per condition/group/cell. All complete vectors and digests agree. The measured
operation still contains request capture, checked mutation, fresh all-root leaf
inputs/layout and fresh complete owned export.

Against `e3c5171`, no complete p50 improves: 41/42 regress and one ties. The
reduction range is -22.64% to 0%; all fourteen large group/cells regress by
4.33–12.24%. P95 has 39 regressions, with a -39.95% to +41.08% range. Max has
29 regressions, with a -352.69% to +92.21% range. Preserve every observation in
the [complete table](2026-11-07-child-rows/comparisons.csv). Indexed rereads,
bounds checks, locality and compiler decisions can offset the removed gather;
this experiment identifies no exclusive cause or allocator-time share.

| Total panes | Candidate / ImGui complete p50, both groups | At most 2x |
| --- | --- | --- |
| 16 | 2.87–4.80x | 0/14 |
| 128 | 1.60–2.07x | 12/14 |
| 1024 | 1.12–1.54x | 14/14 |

The rejected candidate reaches 26/42, while the retained control remains 28/42.
All 42 candidate p50 and p95 cells trail ImGui; 34 max cells also trail it.
This is a measured failed storage/composition experiment, not an architectural
lower bound or completion of the target. Continue compact frames, binary input
storage and the narrower committed weight-validation boundary independently.

The [archive](2026-11-07-child-rows/evidence.tar.xz) contains 434 inputs plus its
manifest, 16,128 raw observations, 336 complete vectors, exact control/candidate
source pairs, ordinary checks/work and source/rebase reviews. Every member
round-trips against its original bytes. The
[decision](2026-11-07-child-rows/decision.json) binds the exact frozen patch and
verifies all three restored source identities. No rejected runtime change is
retained; later candidates must use the retained gather-based composition.
