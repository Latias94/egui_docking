# Reject scalar root issuance after screening

Replacing allocation-backed root references with process-unique scalar
issuances fails the predeclared complete-operation selection. The candidate
is rejected and all three modified files, including temporary tests, are
restored exactly to `c7508ee`. All 324 formal inputs match the retained
control. Original Arc/Weak issuance remains; retained runtime `0e441c8` and
its 30/42 target inventory are unchanged.

The [source and executable assessment](2026-11-07-scalar-issuance/source-review.md)
locates the actual 40-byte issuance allocation beneath timed root completion,
plus old-reference release. That evidence supports a bounded experiment,
without establishing allocation time, exclusive cost or recoverable latency.
It is distinct from deleting one temporary recapture or topology reference.

The candidate keeps `RootReference` opaque and Clone, storing root identity
and a private globally unique serial. A private checked relaxed AtomicU64
issues every new build/edit identity; clones preserve identities, discarded
candidates consume them, and wrapping or recycling is forbidden. Exhaustion
explicitly panics before reuse. This fail-closed exhaustion policy is recorded
as a scope refinement rather than hidden behind an unchecked counter or a
new public Result API. The project already requires AtomicU64.

The existing private RootOwner holds the one-step selection predecessor
serial and full generational Tabs key. Normal edits and changed carriers
clear it, selection records only its immediate predecessor, and unchanged
Move restoration restores both identity and predecessor. The sole selection
consumer queries the current workspace owner at its original stage. No
history directory, cache, extra owner, broad error propagation or public
command/target API is introduced. The
[implementation review](2026-11-07-scalar-issuance/implementation-review.md)
finds no source blocker in issuance, clearing, restoration, old-reference
authority or ordinary error ordering. References and owners are wider, and
the issuer is shared across threads; their costs are not assumed away.

## Semantic qualification and complete comparison

The original `same_version_index` witness is extended with a real Select,
single no-op Tabs Move through general finish, continued admission of the
old index, wrong-Tabs rejection, and two-selection rejection of the initial
index while the intermediate index remains admissible. The no-op enters
restoration but does not create an intermediate new issuance. A separate
authority witness therefore performs real Select, a private candidate root
edit that changes issuance and clears the predecessor, and actual restoration
of both fields. It is a narrow internal owner test, not a claimed public
transaction proving that same intermediate state.

Formatting, 432 targeted tests including the extended existing test, and
four focused regressions pass with normal counters. The new authority test
is outside the original targeted expression and is run separately: one test,
one pass, recorded in `witness-check.json`. Ordinary work matches the full
21-by-20 control CSV byte for byte; every complete vector/digest agrees.
This remains a three-gate early screen plus the additional witness, not
twelve-gate qualification. The other nine gates are required only for a
promising candidate before retention.

Actual retained Rust executable:
`3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`.
Candidate:
`018b13f30ce6c4194cd24e58ed67d36161933a0446264002fd84da2392efeca3`.
The original complete renderer-free timer, 42 group/cells, serial ABBA/BAAB,
48 samples/eight warmups per process/cell and 96 observations per
condition/group/cell remain in both independent comparisons.

| Selection/result | Observed | Predeclared requirement |
| --- | --- | --- |
| Affected move complete p50 improvements | 16/30 | At least 20/30 |
| Small move p50 improvements | 7/10 | At least 6/10 |
| Candidate/ImGui conditions at most 2x | 29/42 | At least 30/42 and all medium/large |

The move threshold and ImGui inventory fail. Whole complete p50 has eighteen
improvements, 21 regressions and three ties, reductions -8.53% to +15.37%.
P95 has 21/20/1, -26.23% to +29.83%; maximum has 19/22/1, -857.95% to
+80.90%. Unaffected resize p50 has 2/8/2. The
[complete table](2026-11-07-scalar-issuance/comparisons.csv) preserves all
negatives. No additional confirmation replaces this failed initial screen.
The rejected candidate's 29/42 does not replace the retained result's 30/42.

The [archive](2026-11-07-scalar-issuance/evidence.tar.xz) contains 429 inputs,
16128 raw observations, 336 complete vectors, exact source/test pairs,
predeclared rules and exhaustion refinement, implementation/design evidence,
original scripts and qualification/work/build/witness receipts. Every member
passes a byte-for-byte manifest round trip. The
[decision](2026-11-07-scalar-issuance/decision.json) records exact restoration.
The [independent evidence audit](2026-11-07-scalar-issuance/independent-evidence-review.md)
recomputes every raw phase/comparison and verifies all vectors, the separate
witness, source restoration and the checksum list. This rejects this
representation and its complete tradeoffs; it does not
prove that heap allocation is free, that scalar identity cannot be faster,
or that Rust ownership imposes the remaining gap.
