# Binary-axis follow-up: two exact treatments rejected

The user authorized continuing after the broad search, specifically testing
same-input, same-complete-output binary solver work. The two bounded treatments
below were qualified and measured independently against actual `a46fdda`
(runtime `0e441c8`), frozen binary
`3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`.
Neither earned retention. All 324 formal source/configuration inputs are restored
exactly; no runtime change or added oracle test survives this follow-up.

| Treatment | Complete p50 improve / regress / tie | Selected small layout improvements | Candidate / ImGui target inventory | Decision |
| --- | --- | --- | --- | --- |
| Reuse the checked weight total | 21 / 20 / 1 | 3/6 | 31/42 | [Reject](2026-11-07-validated-weight-total.md) |
| Expand binary noncentral desired/clamp initialization | 17 / 24 / 1 | 2/6 | 32/42 | [Reject](2026-11-07-binary-desired.md) |

The predeclared rule required at least 28 complete median improvements and four
of six selected layout improvements, with at least 30 target cells and every
medium/large condition still at most 2x. Both treatments miss the first two
conditions. Both have 25 control-relative p95 regressions. All favorable/adverse
p50, p95 and maximum samples remain in their complete comparison tables.

The 31/32 target inventories belong to separately paired ImGui campaigns. Their
control latencies differ from the retained campaign, which has 30/42. They are
not a measured improvement of one retained runtime to 31 or 32, and do not
replace comparison against the unchanged Rust control. No favorable group,
phase or benchmark count was selected to override the declared decision.

## What these experiments actually tested

Validation already computes the checked total weight. Treatment one returned
that same scalar through the private executor and reused it for noncentral
allocation. It removed a duplicate reduction but introduced a longer scalar
lifetime/private argument. Central reductions stayed original. Source analysis
established equivalence; complete measurements did not justify the tradeoff.

Treatment two returned to the actual original parent, without treatment one's
scalar propagation. Only no-central, exactly-two-child desired/clamp initialization
was explicit. It preserved the original checked sum and both expressions
`extent * (weight / weight_total)`, the two clamps and their order, then rejoined
the existing correction/outcome tail. Central and every other arity used the
original fallback. Extra branch/code generation can cost more; its exclusive
causal contribution was not measured.

Neither treatment duplicates a complete numerical executor, alters root topology,
reuses old input or output, changes ownership, or weakens checks. Both preserve
min-before-max aggregate validation, first-error order, exact assigned/grow/shrink,
central saturation and residue logic. They do not adopt ImGui's rounding,
`second = extent - first`, normalized-total assumptions or near-equality returns.

Each passes twelve ordinary counter-enabled gates and the identical 21-by-20
work ledger, plus two serial ABBA/BAAB complete comparisons. Each preserves
16128 raw observations and 336 complete vectors/digests. Existing independent
reference code stays unchanged. A candidate-only witness additionally compares
2904 binary scalar boundary combinations including signed zero, subnormals,
overflow, invalid central index and competing errors. Both candidate tests are
removed with their rejected source patches, with their source and pass receipts
retained in the experiment archives.

## Conclusion and remaining question

This follow-up establishes no retained runtime speedup. The qualified runtime
and its last retained result remain exactly the
[previous checkpoint](2026-11-07-twofold-result.md): 30/42 conditions at most 2x,
with twelve small conditions still above target. It does not prove a complete
specialized binary water-filling algorithm futile, or assign all remaining cost
to binary-versus-general solving. Only the repeated reduction and explicit
initialization hypotheses were tested. Required fresh constraint composition,
candidate preparation and rich output materialization remain separate costs.

A larger binary solver would have to preserve rounding, error priority,
central/nary fallback and one maintained correction authority. No new bounded
source deletion beyond these treatments has been established here. Both [independent review one](2026-11-07-binary-solver-followup/independent-result-review.md)
and [review two](2026-11-07-binary-solver-followup/second-result-review.md) support
closing these two implementation hypotheses without a runtime winner. The
[decision](2026-11-07-binary-solver-followup/decision.json) preserves the missed
numerical goal and limited tested scope. Final publication preserves exact
source restoration and requires pushed-head checks.
