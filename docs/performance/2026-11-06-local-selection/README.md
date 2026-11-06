# Local-selection qualification evidence

The [report](../2026-11-06-local-selection.md) describes the implementation,
eligibility rules, measured gains and remaining costs. The source baseline is
`42fa235b91e36c4cb471532be4548f82e8015fd0`.

Run the standalone archive check from any working directory:

```console
python3 docs/performance/2026-11-06-local-selection/verify_evidence.py
```

The verifier checks SHA-256 entries, recomputes product p50/p95/max from every
raw sample, validates complete case coverage, compares before/after rendered
work and checks all headless output lifecycles. The headless example exposes
process summaries only; individual sample percentiles cannot be recomputed.
`verification.json` records the result before the final hash-manifest check;
running the verifier with the completed manifest produces the same result.

## Contents

- `baseline-provenance.json` identifies the frozen baseline binaries and their
  complete source inventory. The original build receipts are in the linked
  previous archive. Their candidate source inventory matches committed
  `42fa235`; their Git label predates that commit.
- `state.json` records the 13 changed source/test paths and their exact final
  hashes. `source.diff.gz` is the complete patch for those paths, including new
  tests. Eligibility is private to the workspace owner and is not inferred from
  the inventory, Git label or runtime version.
- `candidate-product/` contains the unchanged existing product script's Rust
  and pinned ImGui comparison, build logs, inputs, binary hashes and raw data.
- `frozen-product/` contains one Rust before/after ABBA group and one BAAB group,
  each with two processes per condition. Processes use 24 samples and six
  warmups at 16/128/1,024 panes. Summaries pool actual samples within each group
  and use the nearest-rank percentile convention.
- `headless/` contains eight complete `frame_cost` process summaries, 48 samples
  and six warmups each, and separate allocation runs with six samples and two
  warmups. Timing has inactive DHAT; allocation metrics have active DHAT. All
  update/settlement, idle/selection/drag/measurement/retained-output cases remain.
- `checks.json` and `checks/` contain final-source gate receipts and compressed
  logs. `source_snapshot_sha256` binds every gate receipt to the same recorded
  changed-source inventory. The focused seven tests are included in the full
  workspace count, rather than additional distinct tests.
- `code-review.md` and `code-review-followup.md` retain independent static
  review and recheck. The initial non-blocking qualification-propagation finding
  was fixed before final tests and measurements. Prompts and successful CLI
  exit receipts are included; reviewers did not execute the tests.
- `metrics.json` contains the report's values without rounding.
  `qualification-commands.json` records build intervals. Measurement intervals
  and command arguments are in each workload's `environment.json`.
- `qualify.py` is the experiment driver. It uses the existing product script and
  public headless example; it adds no benchmark hooks or reduced workloads.

## Repeating measurements

Executables are deliberately not committed. Rebuild the baseline revision and
candidate source snapshot in separate clones using the recorded compiler,
manifest, production profile and headless release environment. Freeze the
actual Cargo-reported executables. The source inventories and original binary
hashes allow comparison with this run; a different compiler may produce other
binary hashes and needs its own provenance.

Use a fresh experiment directory for a new run. Copy `qualify.py`, the source
state and baseline provenance there, place frozen executables as
`baseline-product-bin` and `baseline-headless-bin`, and supply successful gate
and review receipts for that exact source snapshot. Run the copied driver from
the candidate repository root. It writes new results beside itself and refuses
to overwrite existing result directories. Its original baseline-hash assertions
should only be updated with the corresponding new-build provenance.

Keep timing serial and separate from compilation, tests, reviews and active heap
profiling. Retain every order group and case, including timing outliers and
unchanged-path variation. Requested allocation bytes represent allocation
requests; live heap, resident memory and elapsed time are separate metrics.
