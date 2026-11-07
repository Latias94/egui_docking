# Binary numeric follow-up: independent result and stopping review

Accept both rejections and close this bounded two-candidate continuation with no runtime winner. Keep immutable `a46fddae12be5a81ac19144f55ab34066a745c5c`, whose runtime is `0e441c8ddeb6f85b9b0e8237303b5def32e178ae`. Neither candidate meets the declared control-relative selection rules. Their 31/42 and 32/42 ImGui threshold counts do not justify retention.

The conclusion is narrower than “binary specialization failed.” This continuation tested duplicate checked-weight-sum reuse and explicit binary desired-size/clamp initialization. It did not evaluate a complete specialized binary constrained water-filling kernel. It also supplies no proof that the remaining target is impossible or that the general algorithm is optimal.

## Results checked

I independently recomputed the following counts and ranges from both frozen `against-baseline/summary.csv` files and recomputed the ImGui threshold counts from `against-imgui/summary.csv`. Improvements are lower complete-operation times; percentage reduction is `(control - candidate) / control`.

| Comparison over 42 group/cells | Checked-total reuse | Binary desired/clamp |
| --- | ---: | ---: |
| Complete p50 improve / regress / tie | 21 / 20 / 1 | 17 / 24 / 1 |
| Complete p50 reduction range | −4.28% to +7.23% | −6.00% to +5.77% |
| Six preselected small-layout improvements | 3 / 6 | 2 / 6 |
| Complete p95 improve / regress / tie | 15 / 25 / 2 | 17 / 25 / 0 |
| Complete p95 reduction range | −17.58% to +30.09% | −40.26% to +10.22% |
| Complete max improve / regress | 21 / 21 | 18 / 24 |
| Complete max reduction range | −522.12% to +79.38% | −317.14% to +81.20% |
| Candidate campaign cells at most 2× ImGui | 31 / 42 | 32 / 42 |

Both miss the predeclared minimum of 28 complete p50 improvements and four of six selected layout improvements. These independent failures are sufficient to reject them; the tails also remain adverse evidence. Maxima are observed campaign tails, not a stable causal attribution. A favorable threshold count in a separate ImGui comparison cannot replace the same-control comparison or retrospectively change the rule. Do not combine favorable cells from the two treatments into an unmeasured winner.

The receipts record twelve successful ordinary gates for each treatment, normal counters enabled for qualification, a matching 21×20 work ledger, and unchanged frozen executables/source bindings. Each treatment's two comparison campaigns retain 16,128 raw observations and 336 complete vectors/digests under the unchanged serial ABBA/BAAB protocol, with 48 samples and eight warmups per process/cell. The recorded complete-state checks pass. This review inspected those receipts and source scopes; it did not independently replay every raw vector or re-audit either archive.

## The actual treatments are independent

The checked-total treatment propagates the already validated scalar through private calls and removes the second noncentral weight reduction. It retains the original min/max and central reductions. Its binary hash is `2eb0e3825e795a47c5d5e4bb59f699388c2d3fcc52927d921f67cb9099bad3dc`.

The measured binary desired treatment is **not** the earlier draft on the checked-total parent. Its actual base is a46. It performs the original checked weight reduction at the original desired-sizing stage, then computes the two original desired expressions and clamps explicitly. The generic fallback and correction/outcome tail remain shared. Its binary hash is `5a53a29bc3373e57c598da0a6a3a20952bebb0bc9aba6c3e8df250f8d270362e`.

Thus the second result cannot be attributed to inheriting the first treatment's scalar parameter flow. Neither campaign identifies an exclusive assembly, ABI, or allocator cause for its regressions. Source-level removal of a reduction or loop does not establish a faster complete operation.

## Is another binary algorithm required now?

No concrete third implementation is required by this evidence. There are untested possibilities, but no newly established hot operation with a justified deletion and expected relevance to the failed cells.

The retained source has explicit limits on what a small fast path could remove:

- `layout.rs:371–397` computes checked minimum and maximum totals before infeasibility outcomes. Moving or eliminating them requires preserving aggregate overflow and competing-error priority.
- `399–402` initializes and clamps desired sizes. Explicit binary initialization was the second rejected treatment.
- `404–429` only calls grow/shrink when the assigned total differs from the extent; exact assignment already skips correction and the second checked assigned sum. An “exact unclamped result” guard does not newly eliminate a water-filling loop on that path.
- `1394–1438` contains the untested general redistribution work: ordered eligibility, eligible-weight reduction, bounded per-child deltas, repeated rounds, and stable residual/zero-weight-capacity consumption. The frozen reference independently preserves that order at `reference.rs:796–840`. Replacing it with a real-number closed form is not sufficient evidence of bit equivalence.

The formal fixture further limits the rationale for specializing the last block. `docking_cost.rs:72–80` builds equal binary splits; `87–103` supplies roots without a central child; `147–159` uses an edge fraction of 0.5; and `215–250` freshly supplies leaf constraints from zero to 65,536. For the unchanged resize trees, the dyadic sizes and broad feasible bounds yield exact half allocations without clamping, so the correction branch is skipped. This is a source-derived workload fact, not a measured branch-frequency percentage. It makes “remove general water-filling rounds” an insufficient explanation for improving those resize cells.

A full fixed-arity executor could still alter code generation or specialize the required validation/composition work. This continuation did not measure that approach. Pursuing it would require a new hypothesis rather than renaming the two rejected treatments as full binary-kernel evaluation.

If that separate work is later authorized, the cheapest useful prerequisite is to establish whether correction/eligibility is actually reached in the target workload, using a separately labeled untimed diagnostic or source-matched symbol/instruction attribution. Only demonstrated work would justify a proposal to eliminate binary eligibility collection/indexing while preserving the original ordered arithmetic, bounds, error checks, and fallback. Such a diagnostic cannot itself establish a speedup. No third candidate or new measurement is proposed for this completed bounded continuation.

## Source and evidence bindings

Both live files match the recorded restored control at review time:

- `crates/dockspace/src/layout.rs`: `01f21abad6c4d71856dbf17ae9eec90a97d7fbe7717bb2e6da87895f2fed5dcd`
- `crates/dockspace/src/layout/topology_tests.rs`: `67671850120413fbaec896b7eea930b7eb52086ccf4864b8486ab21ed8ea1ad2`
- Unchanged `layout/reference.rs`: `4b6ff84c9bed4b65a202e1c771b5f176b161bbb27b99dba25e4875c7b08c07c6`

The reviewed source/test patch hashes are `c4ea86a96b794d8ede5445b8723932154ced2010bc91bec206a5934a011bb422` for checked-total reuse and `ead58ce0f0824e24032e669c8633ab17da14851319d79295fe1b36bb85cbddd2` for actual-parent binary desired.

The four summary SHA-256 bindings, in treatment order and then control/ImGui comparison order, are:

1. `4c4aa5f6c2e42c72d9d98f460a158ed5216fac79244671606ecd13d9f293b1c5`
2. `6dbc6dbe9ae27107e25362f15894bc5adfb514c0cb0b81dd866c403e349abce1`
3. `c03b30ef2c7bd97406dc79944989a7df76dc7597f16f2f6fd5ab0f332dbbf2c7`
4. `57ef30ec5ff1cf2e254c9515d892ddeb40de50d307a179f71c17604981ee1b09`

Only static source/evidence reading, CSV arithmetic, and hashing were performed. No source or candidate was changed; no build, test, runner, timing, profiler, network request, or other independent final opinion was used.
