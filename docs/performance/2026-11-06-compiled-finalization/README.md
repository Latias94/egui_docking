# Compiled-finalization qualification evidence

The [report](../2026-11-06-compiled-finalization.md) describes the opaque compiler
result, retained geometry/authority checks and removed structural reconstruction.
Baseline: `aba4c27b00746de929329fac0a0db3054448a7f6`.

```console
python3 docs/performance/2026-11-06-compiled-finalization/verify_evidence.py
```

The standalone verifier checks SHA-256 entries, exact-source gate receipts,
complete workloads, headless lifecycles and frozen-validator comparisons. It
recomputes product p50/p95/max from raw samples and checks identical before/after
rendered work. Headless runs expose process summaries rather than individual
sample times, so their percentiles cannot be recomputed.

## Contents

- `baseline-provenance.json` binds frozen executables from the previous
  candidate and their complete source inventory to committed `aba4c27`.
  Benchmark and pinned ImGui sources remain byte-identical.
- `state.json` contains the seven final source/test paths and exact hashes.
  `source.diff.gz` is the complete patch, including new regressions. The opaque
  result retains compiler inputs and consuming qualification; no runtime flag,
  independent cache or owned old workspace is introduced.
- `architecture-review.md`, its prompt and exit receipt record the initial
  independent analysis. It identified two accepted-input geometry defects and
  a projection-source counterexample. The implementation preserves checks for
  all three instead of treating finite geometry as sufficient. This review
  targets the baseline design, not the later final diff.
- `code-review.md`, prompt and CLI exit receipt record independent review of the
  actual implementation. `code-review-followup.md` confirms its P2 coverage
  finding is fixed; prompts/exit receipts are included. Reviewers did not
  execute tests or measurements.
- `oracle/` retains the frozen baseline validator and its two row helpers,
  temporary same-input hook and successful log. There were 339 generic and
  789 compiled-path complete-result/error matches across 564 selected tests.
  `oracle-instrumentation.json` records hashes and hook removal. The helper
  recreates the same complete/shared index mode and contained projection.
  These are test differentials, not latency qualification or an exhaustive
  enumeration of all possible geometry. No hook remains in final source.
- `candidate-product/` is the unchanged existing production ThinLTO product
  comparison, Rust versus pinned ImGui, with source inventories, executable
  hashes, all samples and compressed build logs.
- `frozen-product/` contains ABBA and BAAB Rust before/after groups, two processes
  per condition per group, 24 samples and six warmups each, 16/128/1,024 panes.
  Each group's summaries pool its actual samples only.
- `headless/` retains eight complete `frame_cost` process summaries with inactive
  DHAT, 48 samples and six warmups each. Separate active-DHAT runs use six
  samples and two warmups. Update/settlement and idle/selection/drag/measurement/
  retained-output cases are all present.
- `checks.json`, `gate-commands.json` and compressed `checks/` logs bind final
  tests and static checks to one exact source snapshot. Focused tests are
  included in the workspace count, not additional distinct tests.
  `checks-before-coverage-fix.json` retains the earlier attempt; only
  `checks.json` identifies the final source after the review fix.
- `metrics.json` preserves unrounded report values. Build intervals are in
  `qualification-commands.json`; per-workload environment files retain all
  measurement intervals, arguments, executable hashes and result hashes.
- `qualify.py` and `run_checks.py` use existing complete workloads. They add no
  reduced benchmark or permanent production instrumentation.

`verification.json` records verification before the final SHA-256 manifest;
running with the completed manifest gives the same result. Executables are not
committed. Rebuild baseline and candidate in separate clones using the recorded
compiler, manifests and profiles, then freeze Cargo-reported executable paths.
Other compilers can produce different hashes and require new provenance.

For another run, copy the driver to a fresh experiment directory, supply the
new source state, baseline executable provenance, frozen executables and
successful source-bound gate/review receipts. Change original hash assertions
only with matching new-build provenance. Run the driver from the candidate repo
root; it refuses to overwrite result directories. The archived oracle source
uses the original experiment paths and records the exact test patch; reconstruct
it against the recorded baseline before repeating that separate differential.

Keep timing serial and separate from builds, tests, review and active heap
profiling. Keep all order groups, cases and outliers. Requested allocation bytes
are allocation requests, not retained memory or RSS.
