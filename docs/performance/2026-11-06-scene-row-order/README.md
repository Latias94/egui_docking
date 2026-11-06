# Scene row-order qualification evidence

The [report](../2026-11-06-scene-row-order.md) describes reuse of one sorted
row inventory through duplicate checking, semantic joins and canonicalization.
Baseline: `7f959df126701f06ce037386b3caccfce746e6d0`.

```console
python3 docs/performance/2026-11-06-scene-row-order/verify_evidence.py
```

The standalone verifier checks SHA-256 entries, exact-source gate receipts,
complete workload coverage and headless lifecycles. It recomputes product
p50/p95/max from raw samples and checks identical before/after rendered work.
The renderer-free example supplies process summaries, not individual sample
timings, so their percentiles cannot be recomputed.

## Contents

- `baseline-provenance.json` binds the previous experiment's frozen candidate
  executables and complete source inventories to committed `7f959df`. Benchmark
  sources and pinned ImGui inputs are unchanged.
- `state.json` contains the three final source/test paths and their hashes;
  `source.diff.gz` retains their full patch, including the new private helper.
  No public interface, persistent cache or trusted-compiler switch is added.
- `oracle/` retains the frozen old validator, old target helper, experiment-only
  same-input wrapper and successful differential log. `oracle-instrumentation.json`
  identifies the baseline, candidate and temporary-hook hashes. All 668 full
  result/error comparisons across 560 selected tests matched. The temporary
  helper reconstructs the same complete/shared index mode and contained
  projection before invoking the old validator. The hook is absent from final
  source. These tests are not uninstrumented latency measurements.
- `candidate-product/` is the existing product script's production ThinLTO Rust
  and pinned ImGui comparison, including input inventories, raw samples,
  executable hashes and compressed build logs.
- `frozen-product/` contains ABBA and BAAB Rust before/after groups: two processes
  per condition per group, 24 samples and six warmups each, 16/128/1,024 panes.
  Summary percentiles pool actual samples within a group.
- `headless/` retains every complete `frame_cost` process summary, 48 samples
  and six warmups each, plus separate active-DHAT runs with six samples and
  two warmups. All update/settlement, idle, selection, drag, measurement change
  and retained-output cases remain. Timing has inactive DHAT.
- `checks.json`, `gate-commands.json` and compressed `checks/` logs bind final
  checks to the exact same source inventory. Focused tests are included in
  the complete workspace count, not additional distinct tests.
- `code-review.md`, its prompt and CLI exit receipt record independent static
  review. The reviewer did not run tests or measurements.
- `metrics.json` stores exact report values. `qualification-commands.json`
  records build intervals; workload environment files record measurement
  intervals, arguments, executable hashes and result hashes.
- `qualify.py` and `run_checks.py` use existing complete workloads; they add no
  production instrumentation or repository-wide validation framework.

`verification.json` records verification before the final hash manifest;
running with the completed manifest gives the same result. Executables are not
committed. Build baseline and candidate in separate clones with the recorded
compiler, manifests and profiles, then freeze Cargo-reported executable paths.
A different compiler can change binary hashes and requires new provenance.

For a fresh measurement, copy the driver into a new experiment directory and
supply the new source state, matching baseline executable provenance and
successful source-bound gate/review receipts. Original hash assertions may
only change with corresponding new-build provenance. The driver refuses to
overwrite result directories. Run it from the candidate repository root.

Keep timing serial and separate from builds, tests, review and active heap
profiling. Keep every case and order group, including unchanged-path variation
and outliers. Requested allocation bytes are allocator requests, not RSS.
