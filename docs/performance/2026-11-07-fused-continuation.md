# Rejected experiment: fuse projection continuation transitions

Reject the exact standard-Vec control-flow treatment on packed parent-edge
results. It removes two push/pop roundtrips, directly advancing after split
preparation and after the pending child returns. The original fallible order,
dynamic-depth stack, validator, numeric executor, fresh inputs and complete
owned outputs remain. The removed pushes already had available capacity; this
is not an allocation-removal claim.

All twelve ordinary counter-enabled gates pass, including production core,
workspace, public integrations, independent numeric/full-layout oracles, deep
and raw diagnostics, late errors and old-output ownership. The complete
21-row, 20-column work CSV equals the packed control byte for byte. All
vectors/digests agree in both formal campaigns, with serial ABBA/BAAB groups,
48 observations/eight warmups per process/cell and 96 observations per
condition/group/cell. The original complete timer remains unchanged.

Frozen candidate SHA256:
`09f7195d80c0266213485ced3221049be007aaf2af68037d476bbb106dd8b864`.
Actual control is `eb94cb2`, binary
`21888a3b2905da0edce33f68d4a7e3dbf68ba0e45d65f1d72df5d4d03f110435`.
Against that control, complete p50 has five improvements, 36 regressions and
one tie, ranging from -12.44% to +3.38%. Every large cell regresses, by
4.18–7.89%. P95 has 31 regressions, range -293.93% to +57.85%; max has 20
regressions, range -411.16% to +80.47%. The
[complete table](2026-11-07-fused-continuation/comparisons.csv) preserves all
negative and favorable cells. Fewer control transitions do not establish a
faster complete operation; no exclusive instruction/compiler cause is claimed.

| Total panes | Complete p50 candidate / ImGui, both groups | At most 2x |
| --- | --- | --- |
| 16 | 2.1244–3.9328x | 0/14 |
| 128 | 1.3482–1.7590x | 14/14 |
| 1024 | 0.9147–1.2449x | 14/14 |

The result remains 28/42 and misses every small cell. Against ImGui, 40 p50,
38 p95 and 24 max cells trail; two large four-view edge medians lead. This
mixed comparator outcome does not overcome regressions against the actual
retained control or satisfy the target.

The [archive](2026-11-07-fused-continuation/evidence.tar.xz) preserves 462 inputs
plus manifest, 16,128 raw observations, 336 complete vectors, exact measured
control/candidate source, full ordinary/work checks, source review and separate
unexecuted historical source packages. The
[decision](2026-11-07-fused-continuation/decision.json) binds the frozen patch
and verifies that reversing only that patch restores the full layout source
byte for byte to `eb94cb2`. No SmallVec or other queued treatment is included.

Continue the separately reviewed numeric hint, exact unchanged-total reuse,
axis compiler-boundary, persistent directory and binary measurement gathering
candidates on their actual retained controls. No stopping conclusion follows.

The [independent evidence review](2026-11-07-fused-continuation/evidence-review.md)
reproduces the complete statistics, qualification, work, vectors and immutable
restoration. Every medium as well as large median regresses. The displayed
large range now uses standard two-decimal rounding; exact data are unchanged.
