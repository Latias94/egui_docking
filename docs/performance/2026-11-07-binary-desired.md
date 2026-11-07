# Rejected explicit binary desired/clamp initialization

Reject the independent fixed-two-child initializer against immutable `a46fdda`
(runtime `0e441c8`). The guard is exactly no central child plus two weights,
constraints and output slots. It runs the original checked weight sum at the
original desired stage, calculates both original `extent * (weight / total)`
expressions, then clamps both in original order. It rejoins the one existing
assigned/grow/shrink/residue/outcome tail. Central/nary initialization stays in
the original generic fallback. Complete validation and min-before-max checks
remain before this branch. No normalized-weight assumption or fixture guard is
added. No arithmetic is reassociated and no result is derived by subtraction.

This actual candidate is based directly on `a46fdda`; the rejected weight-total
propagation and additional scalar ABI are absent. The earlier draft on that
unqualified parent is not its actual source binding. Independent review covers
the rebased and rustfmt-adjusted real delta. The unchanged frozen numerical
reference remains independent. One boundary witness checks 2904 binary
combinations by successful output bits and first-error diagnostics.

All twelve ordinary gates pass with normal counters, including 433 targeted,
2074 production core, 2250 workspace, four public integrations, 292/335 host and
ten doc tests, alongside format, strict docs, dependency and lint gates. The
full 21-by-20 work ledger matches actual control byte-for-byte. Two serial
ABBA/BAAB groups preserve the original capture, checked mutation, fresh all-root
leaf/layout and complete owned export timer, 48 samples/eight warmups per
process/cell and 96 observations per condition/group/cell. All complete vectors
and digests match. Candidate SHA-256:
`5a53a29bc3373e57c598da0a6a3a20952bebb0bc9aba6c3e8df250f8d270362e`.

Complete p50 has seventeen improvements, 24 regressions and one tie, reductions
-6.00% to +5.77%. P95 has seventeen improvements and 25 regressions, range
-40.26% to +10.22%. Max has eighteen improvements and 24 regressions, range
-317.14% to +81.20%. Every adverse and favorable observation is in the
[complete table](2026-11-07-binary-desired/comparisons.csv).

The predeclared rule required at least 28 complete median improvements and at
least four improvements in the six selected single-view small layout cells.
Both fail: 17 complete and two selected layout improvements. The candidate's
32/42 inventory in this separate ImGui campaign cannot replace those failed
control-relative conditions or be combined with another campaign's comparator
latency. Reject both changed source/test files and restore their exact immutable
control bytes. No production candidate or new oracle test is retained.

The [archive](2026-11-07-binary-desired/evidence.tar.xz) roundtrips 431 inputs,
16128 raw observations, 336 full vectors, both original ordinary invocations and
the verified combined twelve-gate receipt, source/review pairs and predeclared
scope. The [decision](2026-11-07-binary-desired/decision.json) binds restoration.
The explicit scalar initializer's additional branch/code footprint may cost
more; no exclusive compiler cause or allocator-time share is established.

This experiment tests fixed-arity desired/clamp initialization, not a complete
alternative binary water-filling algorithm. It does not establish that every
binary algorithm is slower, that remaining optimization is impossible, or that
Rust has an inherent penalty. The
[independent full evidence audit](2026-11-07-binary-desired/independent-evidence-review.md)
finds no binding, completeness or rejection issue. All 431 inputs, gates, raw
observations, full vectors and exact restoration were independently checked.
