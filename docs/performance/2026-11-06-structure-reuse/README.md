# Structure-reuse qualification evidence

The [report](../2026-11-06-structure-reuse.md) describes reuse of immutable
workspace facts, layout topology, measured results and remaining repeated proofs.
Baseline: `f4b12d7ea88d561d717fd3898de8f81c4ce71785`.

Run the standalone archive check from any working directory:

```console
python3 docs/performance/2026-11-06-structure-reuse/verify_evidence.py
```

The verifier checks SHA-256 entries, recomputes all product p50/p95/max values
from actual samples, checks complete workload coverage and equal before/after
rendered work, and verifies every headless output lifecycle. The headless
example exposes process summaries; individual sample percentiles cannot be
recomputed. `verification.json` records the check before the hash manifest is
created. Running it again with the final manifest produces the same result.

## Contents

- `baseline-provenance.json` identifies the frozen baseline executable hashes
  and source inventory. They match the previous experiment's candidate and
  committed `f4b12d7`, including the complete benchmark/reference inputs.
- `state.json` records the seven changed source/test paths and exact final
  hashes. `source.diff.gz` includes the complete patch and new tests. Its
  inventory identifies the source snapshot; it grants no runtime authority.
- `candidate-product/` contains the existing unchanged product comparison
  script's production ThinLTO build and pinned ImGui comparison, build logs,
  raw samples, source inventories and executable hashes.
- `frozen-product/` contains actual before/after ABBA and BAAB groups, each with
  two processes per condition, 24 samples and six warmups per process, at
  16/128/1,024 panes. Percentiles pool actual samples only within a group.
- `headless/` retains eight complete renderer-free process summaries with
  inactive DHAT, 48 samples and six warmups each, plus separate active-DHAT
  allocation runs with six samples and two warmups. Update, settlement, idle,
  selection, drag, measurement change and retained-output cases remain intact.
- `diagnostic/` retains temporary baseline instrumentation, its patch and
  source hashes, probe, raw spans, summaries, environment, build log and stderr.
  No diagnostic hook remains in production source. Instrumentation perturbs
  duration, parent scopes include nested scopes, and absent spans mean skipped
  work. Use it for attribution, not uninstrumented latency claims. The included
  `diagnostic/summarize_stages.py` is an unchanged copy of the helper in the
  previous `2026-11-06-selection-validation` archive. The instrumentation script
  records how the baseline was patched; its remove mode needs the original
  experiment's saved source bytes, which are not duplicated here.
- `checks.json` and `checks/` contain final-source receipts and compressed logs.
  Each receipt binds to the same final changed-source inventory. The nine
  focused tests, including four new tests and expanded existing cases, are
  included in the full workspace test count.
- `architecture-review.md` and `code-review.md` are independent static reviews.
  Their prompts and successful CLI exit receipts are included. Reviewers did
  not run tests or benchmarks. `code-review-resolution.md` records the sole
  non-blocking test finding and its subsequent fix and rerun. The pre-fix
  attempt stopped at formatting; its receipts are separate from final gates.
- `metrics.json` contains unrounded report values. `qualification-commands.json`
  records build intervals; workload `environment.json` files record every
  measurement interval, command, executable hash and result hash.
- `qualify.py` and `run_checks.py` are local experiment drivers using existing
  workloads. They add no reduced benchmark, production test hook or repository
  validation framework.

## Repeating measurements

Executables are not committed. Rebuild the baseline and candidate source in
separate clones with the recorded compiler, manifests and profiles; freeze the
actual Cargo-reported executable paths. A different compiler can change binary
hashes and needs new provenance.

Copy the experiment drivers to a fresh directory, supply the exact source state,
new baseline executable provenance, frozen baseline executables and successful
source-bound gate/review receipts, then run the driver from the candidate repo
root. Update original hash assertions only with corresponding new provenance.
The driver refuses to overwrite result directories.

Keep measurements serial and separate from compilation, tests, review and
active heap profiling. Keep all cases and order groups, including outliers or
unchanged-path variation. Requested allocation bytes are allocation requests,
not RSS or resident memory.
