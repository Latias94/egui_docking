# Checked-Move evidence

This archive supports [the result](../2026-11-06-checked-move.md) and
[the plan](../../plans/2026-11-06-002-checked-move-refactor.md). Base:
`a2bcabc1440caa664fb5f4a64c4669c4eecfdce4`.

| Directory | Meaning | Raw samples | Group summary rows | Full sample-zero vectors |
| --- | --- | ---: | ---: | ---: |
| `frozen-comparison/` | First candidate vs frozen baseline; four-view gate not met | 8,064 | 84 | 168 |
| `candidate-reference/` | First candidate vs pinned ImGui | 8,064 | 84 | 168 |
| `frozen-comparison-v2/` | Final candidate vs same frozen baseline | 8,064 | 84 | 168 |
| `candidate-reference-v2/` | Final candidate vs pinned ImGui | 8,064 | 84 | 168 |
| `resize-followup/` | Same final/baseline executables, resize only, 96 samples/16 warmups | 4,608 | 24 | 48 |

Each dataset contains its environment receipt, raw CSV and group summaries.
Final datasets additionally contain pooled phase sums/shares and percentiles.
All percentiles use nearest rank; all samples, p95s and maxima are retained.
Groups are serial ABBA then BAAB. Frozen comparisons reuse the unchanged
renderer-free probe and its complete request/mutation/all-root-layout/fresh
owned export endpoint. Logical views do not include native OS viewport
lifecycle or rendered UI finalization. Rust's stronger transaction and richer
internal projection contract remain included.

`sample-zero-states.tar.gz` preserves every process's complete normalized state
files under its original process-directory name. `process-output.tar.gz`
preserves each process's original CSV/stdout/stderr. Tar members are relative
regular files with deterministic headers. Raw CSVs and build/check logs use
gzip. Decompress when checking an environment's original uncompressed result
hash. `SHA256SUMS` instead hashes the actual stored archive bytes.

All corresponding sample-zero vectors match; every sample's complete-state
digest also agrees at the same input dimensions. The second resize width has
per-sample digest and the probe's complete outside-timer oracle, but no separate
sample-zero full vector. Every raw row's four phase durations sum exactly to
its direct operation-owned total.

`work-counts.csv` and `work-counts-v2.csv` are independent ordinary-counter-build
results and are byte-identical. They reset after setup and snapshot before
external validation; their instrumented times are discarded. Full candidate
clone/item-scan inventories remain unchanged by the second increment.

`v1-source-copies.tar.gz` retains the exact seven changed source files of the
first measured candidate, including the original graph implementation. Every
copy matches that candidate environment's hash. The final source is in the
commit. `reviewed-runtime.json` is the earlier six-file v1 review receipt, not
the final candidate source list. `baseline-build-environment.json` and
`baseline.json` connect the frozen baseline executable to the earlier qualified
build; the executable SHA-256 is the same in every before-after run.

The one-off `compare_frozen-v1.py`, `compare_frozen.py` and
`compare_resize_followup.py` retain the exact local orchestration. They use
the unchanged portable comparison script's CSV validator and percentile helper;
their explicit local paths are evidence of this experiment, not a new project
CLI. Reproduce the current candidate/ImGui endpoint with:

```text
python3 scripts/compare_docking.py --out /tmp/dockspace-checked-move-reference
```

Design, implementation, recovery, simplification and evidence review reports
are included at archive root. Reports that predate the final two regressions or
the second increment state their exact snapshot and do not substitute for the
final root-run logs. `checks.json` and `checks/` retain those results, including
the first obsolete-counter assertion failure and every subsequent complete
passing suite. No failed first iteration is relabeled as a successful final
gate, and no resize/tail regression is removed.

Runtime executables remain under
`/tmp/dockspace-checked-move-20261106-a2bcabc/` and are not committed. This archive
does not claim complete rendered product improvement, allocation event/byte or
RSS savings, universal tail improvement, or a theoretical residual performance
floor.
