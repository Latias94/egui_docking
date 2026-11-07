# Rejected reuse of the validated weight total

Reject the standalone scalar-reuse treatment against immutable `a46fdda`
(runtime `0e441c8`). `validate_inputs` returned its existing checked weight
sum through the two private callers into the sole numerical executor and
noncentral desired sizing. The second identical checked sum was removed.
All checks and central sibling/desired reductions retained their original order.
No alternate solver, dependency, public API, output or formal probe changed.

The independent contract review found no source blocker. One new test compares
2904 binary boundary combinations against the unchanged frozen reference,
including signed zero, subnormals, nonfinite weights, sum overflow and competing
errors. Successful output sizes/outcomes compare by bits. All twelve ordinary
counter-enabled gates pass: 433 targeted, 2074 production core, 2250 workspace,
four public integration, 292/335 host and ten documentation tests, plus format,
strict docs, dependency and correctness/suspicious lint gates. The full 21-by-20
work ledger matches control byte for byte.

The unchanged complete-operation protocol uses two serial ABBA/BAAB groups,
48 samples/eight warmups per process/cell and 96 observations per condition/group.
All 16128 raw observations and 336 full vectors/digests agree. Candidate binary:
`2eb0e3825e795a47c5d5e4bb59f699388c2d3fcc52927d921f67cb9099bad3dc`.
Control binary: `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`.

Complete p50 has 21 improvements, twenty regressions and one tie, reductions
-4.28% to +7.23%. P95 has fifteen improvements, 25 regressions and two ties,
range -17.58% to +30.09%. Max has 21 improvements and 21 regressions, range
-522.12% to +79.38%. Every favorable and adverse cell remains in the
[complete table](2026-11-07-validated-weight-total/comparisons.csv).

The predeclared selection required at least 28 complete median improvements.
That condition fails. The selected six layout comparisons improve only three,
also below the predeclared four-cell requirement. The candidate reaches 31/42 against ImGui in this
campaign; that threshold count does not replace control-relative selection.
Both modified source/test files are restored to the exact immutable control.
This is not evidence that checked weight reuse is intrinsically slow, or that
its added scalar ABI uniquely caused these observations.

The [archive](2026-11-07-validated-weight-total/evidence.tar.xz) preserves
432 inputs, all ordinary/work checks, exact source/review pairs, measurement
scripts, raw observations and full vectors. Targeted and remaining ordinary
checks were separate invocations on identical hashes; their original receipts
and verified combined twelve-gate receipt remain. The
[decision](2026-11-07-validated-weight-total/decision.json) records exact
restoration. The next independent treatment returns to the actual retained
parent and only unfolds two-child noncentral initialization into the same
existing numerical correction tail. It does not inherit this rejected scalar
propagation. The
[independent full evidence review](2026-11-07-validated-weight-total/independent-evidence-review.md)
finds no blocking binding or rejection issue. It recomputed every summary and
checked all 432 inputs, gates, raw observations, vectors and exact restoration.
