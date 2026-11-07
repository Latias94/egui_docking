# Reject insertion-aware Tabs copying after the performance screen

Copying a shared Tabs payload directly into space for its next insertion
does not improve the declared complete-operation workload sufficiently.
The candidate fails its initial affected thresholds and is rejected without
further ordinary gates or confirmation. All three modified files, including
the extended semantic witness, are restored exactly to `c7508ee`; all 324
formal inputs match retained runtime `0e441c8` and its 30/42 target inventory.

The [current-source location review](2026-11-07-tabs-insertion-cow/source-review.md)
binds the existing clone-then-grow mechanism: shared target Tabs clones both
owned vectors at their old lengths, then the actual insert immediately grows
visual items and MRU. The source-matched sample's `insert_items +800` reserve
belongs to visual splice; MRU has a separate growth site. These observations
support the experiment, not an allocator-time share or benefit forecast.

A private `NodeStore::get_mut_for_tab_insert` handles only an actual nonzero
insertion into shared, nonempty Tabs with a valid index. It copies both old
slices into ordinary Vecs with capacity for the additional entries, preserving
selection, then returns through safe `Arc::make_mut`. Unique/weak-only,
empty-target, invalid-index and wrong-kind paths retain ordinary behavior.
`insert_items` changes only its initial mutable access; the original splice,
selection, MRU, validation/error order and transaction publication remain.
The old Node's payload-copy counter is recorded once before manual copying.
No allocator abstraction, new container, alternate merge algorithm or public
API is introduced.

The [independent implementation review](2026-11-07-tabs-insertion-cow/implementation-review.md)
finds no source blocker. The original shared-copy witness remains 2 payloads,
4 visual items and 4 MRU entries. Its unique followup gains a real Open of
Item 99, verifying zero new payload copies, exact complete visual/MRU state
and the original workspace's unchanged contents. It asserts no capacities.
The candidate still introduces eligibility/strong-count branches, safe Arc
access and less spare capacity for later staged inserts; those counter-costs
are not assumed away.

## Early qualification and complete comparison

Three normal-counter gates pass: formatting, 432 targeted tests including
the extended witness, and four focused regressions. The complete 21-by-20
work CSV remains byte-identical, and all complete vectors/digests agree.
Under the predeclared early-screen protocol the other nine ordinary gates
run only for a promising candidate before retention. They are not run here;
twelve-gate qualification is explicitly not claimed.

Actual control binary:
`3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`.
Candidate:
`291bd6b54bd8a655ef041713f4fad71d0c11dbde6dc894fb828628419875e700`.
Both full campaigns preserve the original 42 group/cells, renderer-free
complete-operation timer, fresh inputs/results and serial ABBA/BAAB protocol
with 48 samples/eight warmups per process/cell, 96 observations per
condition/group/cell.

| Selection/result | Observed | Predeclared requirement |
| --- | --- | --- |
| Center/cross-view complete p50 improvements | 2/18 | At least 12/18 |
| Small center/cross-view improvements | 1/6 | At least 4/6 |
| Candidate/ImGui conditions at most 2x | 30/42 | At least 30/42 and all medium/large |

The first two conditions fail decisively. Whole complete p50 has eight
improvements, thirty regressions and four ties, reductions -13.82% to +4.99%.
P95 has 18/24/0, -283.60% to +15.47%; maximum has 13/29/0, -751.32% to
+85.31%. All observations remain in the [complete table](2026-11-07-tabs-insertion-cow/comparisons.csv).
The separate ImGui target count does not override the actual-control result
or represent a retained performance improvement.

The [archive](2026-11-07-tabs-insertion-cow/evidence.tar.xz) preserves 427
inputs, 16128 raw observations and 336 complete vectors, exact base/candidate
source, the semantic witness, three-gate/work/build receipts, scripts and
the design/implementation reviews. The complete manifest round trip checks
archived bytes. The [decision](2026-11-07-tabs-insertion-cow/decision.json)
records rejection and exact restoration. The
[independent evidence audit](2026-11-07-tabs-insertion-cow/independent-evidence-review.md)
recomputes every comparison and verifies the early-screen boundary and
restored formal inputs; the completed SHA256SUMS includes the decision.
This result rejects this private
copy-capacity tradeoff; it does not establish a language/ownership lower
bound or that all intended-capacity copying is harmful.
