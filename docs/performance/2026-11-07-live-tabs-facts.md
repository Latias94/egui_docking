# Reject borrowed Live Tabs fact comparison after screening

The Live Tabs target validator can avoid a second checked target capture and
its temporary owned reference. This concrete deletion nevertheless fails the
predeclared complete-operation thresholds. All three modified files, including
the new error-priority witness, are restored exactly to `c7508ee`. All 324
formal inputs match the retained control; runtime `0e441c8` and its 30/42
target inventory remain unchanged.

The [source and executable review](2026-11-07-live-tabs-facts/source-review.md)
locates the repeated membership/issuance checks and success-path Arc
retain/drop in the actual timed checked-move path. The compiler has not
eliminated them. The temporary reference shares an existing allocation; this
is an atomic ownership deletion, not an allocator saving. No time share or
complete-operation benefit follows from the location evidence alone.

The candidate replaces the private `ReferenceAuthority::capture_tab_target`
wrapper with a matcher. Live validation retains its strict reference check
in the original position, then derives current surface, central status and
pane rule through the existing queries before comparing their Copy values.
The workspace query gains only crate visibility; its body is unchanged.
Indexed validation still captures and compares the complete target, and
both edge paths retain their original implementation. No authority token,
cache, provider or new target representation is introduced.

The [independent implementation review](2026-11-07-live-tabs-facts/implementation-review.md)
finds no blocker. The candidate's paired real-transaction witness combines
a current wrong-kind target with forged surface and an invalid gap, expecting
`NodeIsNotTabs`; its valid-Tabs case expects the frozen-facts invariant before
gap bounds. Both verify exact rollback. These protect the error-order risk
of comparing scalar facts too early. Stale-reference error payload ownership
and the initial full-generational reference validation remain unchanged.

## Early qualification and comparison

Three normal-counter gates pass: formatting, 433 targeted tests including the
new witness, and four focused regressions. The full 21-by-20 work CSV is
byte-identical to the control. Complete output vectors and digests agree in
both comparisons. This is an early performance screen; the remaining nine
ordinary gates and one additive Rust confirmation are required only for a
promising candidate before retention. Neither is claimed here.

Actual Rust control executable:
`3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`.
Candidate:
`903b610a46bd8e7a1f4250b3251f9aebe2af14e6e600285062e0f2de33081636`.
The original renderer-free timer includes capture, checked mutation, fresh
all-root layout and complete owned export. Both comparisons retain all 42
group/cells, serial ABBA/BAAB order, 48 samples/eight warmups per process/cell
and 96 observations per condition/group/cell.

| Selection/result | Observed | Predeclared requirement |
| --- | --- | --- |
| Affected center/cross-view complete p50 improvements | 4/18 | At least 12/18 |
| Small center/cross-view improvements | 1/6 | At least 4/6 |
| Candidate/ImGui conditions at most 2x | 30/42 | At least 30/42 and all medium/large |

Both affected thresholds fail. Whole complete p50 has ten improvements,
thirty regressions and two ties, reductions -10.40% to +5.77%. P95 has
20/22/0, -29.02% to +32.58%; maximum has 19/22/1, -389.47% to +69.90%.
Every result remains in the [complete table](2026-11-07-live-tabs-facts/comparisons.csv).
The separate candidate/ImGui inventory does not override the Rust-control
comparison. No additional campaign replaces the initial negatives.

The [archive](2026-11-07-live-tabs-facts/evidence.tar.xz) preserves 426 inputs,
16128 raw observations, 336 complete vectors, exact base/candidate sources,
the test, predeclared thresholds, original scripts, design and implementation
reviews and three-gate/work/build receipts. All archived members pass the
byte-for-byte manifest round trip. The
[decision](2026-11-07-live-tabs-facts/decision.json) records rejection and
exact restoration. The
[independent evidence audit](2026-11-07-live-tabs-facts/independent-evidence-review.md)
recomputes the raw comparisons, full vectors, work and source bindings and
accepts rejection; the completed checksums include the decision. This rejects
this particular borrowed-fact tradeoff; it
does not establish that redundant checks or reference-count operations are
free, identify a compiler cause for the regressions, or establish a lower
bound on the remaining difference.
