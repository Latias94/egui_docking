# Selection and validation evidence

See the [analysis and measured change](../2026-11-06-selection-validation-costs.md).
Production baseline: `eea8a7f8798187214b6fd086585906db67c99036`.
The only production changes are the two paths and hashes in `state.json`:
four redundant validator calls are removed and the existing canonicalization
postcondition is documented. `source.diff` captures that complete production diff.

## Experiments

| Directory/artifact | Purpose | Boundaries |
| --- | --- | --- |
| `reproduction-one` | Recheck the previous dense-layout product idle result | Original frozen binaries, only 1,024 panes, 48 samples/12 warmups; one ABBA and one BAAB group |
| `reproduction-original` | Restore the original full product workload parameters | Same binaries, 16/128/1,024 panes, 24 samples/six warmups; one ABBA and one BAAB group |
| `stages-*`, `instrumentation.*` | Attribute selection's cost to actual core stages | Temporary spans in the original public headless workload; 32 samples/six warmups at all scales; raw spans and trace retained |
| `imgui-assert-comparison` | Check the scale of the ImGui assertion-build difference | Only remove `-DNDEBUG` from the pinned production compiler command; 128 samples/16 warmups at 1,024 panes; one ABBA and one BAAB group |
| `validation-candidate-product` | Run the normal complete Rust/ImGui product comparison after the minimal change | Original production script, three scales, 24 samples/six warmups, Rust/ImGui ABBA |
| `validation-frozen-product` | Compare frozen `eea8a7f` and minimal-change Rust products directly | Original three-scale workload, 24 samples/six warmups, one ABBA and one BAAB group |
| `validation-headless` | Qualify the minimal change without either UI renderer | Unchanged frame workload at all three scales; 48 samples/six warmups timing, six samples/two warmups in separate active-DHAT allocation runs |

The narrower idle reproduction changes several parameters. It cannot isolate
warmup or scenario order as a cause. Both reproductions retain all scenarios
emitted by the executable. The prior idle regression remains unresolved rather
than declared fixed.

Stage spans are diagnostic. Parent spans include nested children and diagnostic
printing; they are not exclusive CPU costs. Per-update calls are summed before
percentiles are computed. Do not add nested spans or independently measured
medians. `stages.csv` retains the original harness summaries; `stages-raw.csv`
contains the parsed spans for all 32 warm measured updates of each reported case
and scale. All other diagnostic cases remain in `stages.stderr.gz`.

The original 12 instrumented source files were restored exactly after freezing
the diagnostic executable. `instrumentation.json` retains before/after hashes;
`instrumentation.diff` describes the probes. The later production change is
separate. No diagnostic hook or feature remains in the shipping source.

The earlier product/headless executables are the frozen candidates from the
[compiled-layout experiment](../2026-11-06-compiled-layout/README.md), matching
the current production baseline. Absolute executable paths, source hashes and
timestamps are retained as recorded, including `/private/tmp` aliases on macOS.
Large executables are preserved in the local experiment directory identified in
`provenance.json`, not committed here.

## Checks and reviews

`checks.json` and `checks/*.log.gz` contain all ten serial regression gates.
Build/check logs and the diagnostic stderr are gzip-compressed with their exact
uncompressed bytes. Other artifacts retain original text bytes.

`minimal-review.md` is the complete independent read-only review of the shipped
two-file change. It found no blocking issue and checked the full validator call
chain, lack of intervening mutation, error wrappers and rollback/reconciliation.
`design-review.md` is advisory analysis for larger future work; its final sentence
is truncated in the original captured output and is preserved that way. It is
not the correctness receipt for the shipped diff.

No builds, tests, sampling, diagnostic tracing or reviews overlap the formal
assertion-comparison or production qualification timing windows. Build provenance
is in `imgui-assert-build.json`, `instrumented-environment.json`, product
environment files and `qualification-commands.json`. Workload sources are
unchanged.

## Recompute

From the repository root:

```text
python3 docs/performance/2026-11-06-selection-validation/helpers/verify_evidence.py docs/performance/2026-11-06-selection-validation
```

This checks complete per-process raw-sample coverage, process-file equality,
equal product workload counts, nearest-rank p50/p95/max and the two-file source
delta. It recomputes all exposed diagnostic-stage percentiles and call counts.
The unchanged headless harness exposes per-process timing/allocation summaries,
so those checks verify coverage and hashes, not unavailable individual samples.
The deterministic output is `verification.json`.

The `helpers` directory contains only this experiment's orchestration and
temporary instrumentation. The orchestration retains original local paths and
expects the frozen binaries and saved original files in that local experiment;
it is not a repository release or generic benchmark interface. For a fresh
product measurement, use the existing command:

```text
python3 scripts/compare_products.py --backend fork --profile production --samples 24 --warmup 6 --panes 16,128,1024 --runs 4 --out /tmp/validation-product-rerun
```

Build the exact baseline in a clean separate checkout when repeating the
before/after experiment; freeze and hash both actual executables before
interleaving them. All compilation and measurements run serially. `SHA256SUMS`
covers every archive file except itself. No sample was discarded for its timing.
