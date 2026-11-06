# Compiled layout evidence

The [report](../2026-11-06-compiled-layout.md) explains the implementation,
measurement results and unresolved full-product idle regression. Baseline:
`f28a381499455c252b8772ed772340f6dff9072a`. The candidate differs only in the six
source paths listed in `state.json`; `source.diff` includes their complete diff.
The workload sources and pinned ImGui inputs are unchanged.

## Contents

- `baseline-layout`, `candidate-layout`: the existing renderer-free comparison
  script, two Rust/ImGui ABBA groups each, 96 samples and 16 warmups per case per
  process, 16/128/1,024 leaves. Every node's common-subset geometry is checked.
- `baseline-product`, `candidate-product`: the existing fork production product
  script, Rust/ImGui ABBA, 24 samples and six warmups, the same three scales.
- `frozen-layout`: frozen before/after Rust executables, two ABBA groups, the
  unchanged full/prepared cases and workload. Preparation is reported separately.
- `frozen-product`: frozen before/after product executables, two ABBA groups.
- `frozen-product-reverse`: two further BAAB groups to investigate the observed
  idle regression. Sources, binaries, workload and sample counts are unchanged.
  These results remain in the report along with the initial measurements.
- `headless-investigation`: the unchanged public core frame workload at 1,024
  leaves, one ABBA and one BAAB timing group, 48 samples and six warmups per
  process; separate active-DHAT runs use three samples and one warmup. This
  harness supplies process summaries, not individual samples. Its baseline
  binary comes from the earlier shared-manifest experiment, whose 363 source
  inputs match the current baseline exactly.
- `checks`, `checks.json`, `post-review-checks.json`: full and post-review gates.
- `independent-review.md`, `review-*`, `oracle-red.json`: independent read-only
  review and closure of its P3 test-oracle finding with a reproducing regression.
- `state.json`, `provenance.json`: baseline/current source hashes and scope.
- `helpers`: one-off orchestration and verification used for this experiment;
  these do not extend the repository's release or benchmark interfaces.

Each layout/product directory retains exact raw CSV rows, aggregate CSV,
process stdout/stderr, commands, compiler versions and binary/input hashes.
Environment files retain absolute paths and timestamps from the actual runs.
Build and check logs are gzip-compressed without changing their uncompressed
bytes. Large executables are retained only in the local experiment directory
named by `provenance.json`, not committed here. The archive's Git attributes
preserve raw text bytes and the whitespace of captured stdout, source diff and review.

## Recompute the evidence checks

From the repository root:

```text
python3 docs/performance/2026-11-06-compiled-layout/helpers/verify_results.py docs/performance/2026-11-06-compiled-layout
```

This recomputes nearest-rank p50/p95/max from all layout/product raw samples,
checks complete per-process coverage and process-file equality, and compares
each product sample's workload counts with the baseline. It also checks geometry
digests across versions and ImGui, and the unchanged workload/source inventory.
The headless checks verify complete process/metric coverage and file hashes;
they do not reconstruct unexposed individual timings. The verification result
is `verification.json`.

## Reproduce the measurements

The repository's existing commands are sufficient for a fresh candidate run:

```text
python3 scripts/compare_layouts.py --out /tmp/compiled-layout-rerun
python3 scripts/compare_products.py --backend fork --profile production --samples 24 --warmup 6 --panes 16,128,1024 --runs 4 --out /tmp/compiled-product-rerun
```

Run commands serially on an otherwise quiet machine. The scripts enforce one
Cargo build job and build before timing. Rebuild the baseline from the named
commit in a separate clean checkout, then preserve and hash both versions'
actual executables before crossover measurements. The helper scripts describe
the exact original local orchestration and expect those original experiment
paths; they are not general benchmark commands. Current source hashes must
match before using them.

`SHA256SUMS` covers every archive file except itself. `verification.json` is
deterministic when recomputed from an unchanged archive. No baseline or candidate
sample was dropped because of its timing result.
