# Compilation scratch work reduction

Date: 2026-11-05. Status: complete. Base: `4484e40`.
Implementation: `5130dba`.

## Outcome and constraints

Reduce complete changed-update construction costs on the freshly measured
implementation. Preserve full ordered output recording, exact geometry and
identities, all semantic checks and their rejection order, immutable historical
outputs, atomic publication, native first-live ownership and the sole canonical
graph and presentation ledger. Keep all pre-existing CI and review changes.
No remote publication is included.

## Evidence and selected work

- [x] Verify the release executable and all 551 previous source inputs against
  the previous measurement record; freeze the baseline executable.
- [x] Collect fresh full-workload CPU samples and a separate measurement-change
  profile without active heap instrumentation. The latter avoids attributing drag
  press setup costs to the measured changed update.
- [x] Finish the unchanged complete 12-sample/three-warmup timing and heap baseline
  at 16/128/1,024 panes, with three older retained outputs.
- [x] Borrow immutable hit records during touch compilation, reuse each handle's
  initial splitter lookup for its anchor, and emit surviving base fragments
  directly. Use a call-local splitter lookup that binary-searches only strictly
  canonical records and retains linear first-match behavior otherwise.
- [x] Preallocate surface-program uniqueness scratch from the existing rosters
  and store immutable references instead of copying large identities. Preserve
  duplicate-category and first-observed error order and stable paint ordering.
- [x] Build the expected magnetic catalog in one temporary vector, sort and
  deduplicate once, and compare every complete ID with the existing sorted target
  index instead of constructing three transient ordered sets.
- [x] Run focused and complete serial verification, crossed independent review,
  full comparable measurements, source inventory and scoped local commits.

## Results

The [complete performance record](../performance/2026-11-05-compilation-scratch-results.md)
contains the adjacent baseline, final CSV, earlier baseline, all supplementary
interleaved small-workload timings, environment and source inventory. At
1,024 panes changed-update p50 falls 8.0–10.2%, and requested allocation traffic
falls 6.1–6.6%, with identical live checkpoints. A frozen-baseline repeat after
the final run checks the long gap from initial sampling. Four interleaved
timing-only processes do not reproduce the observed 128-pane measurement-change
p50 regression; both the original and follow-up observations remain recorded.

All 2,085 workspace tests and the complete public consumer/native, Clippy,
documentation, formatting and dependency gates pass serially. Two independent
read-only reviews find no evidenced P1/P2. Changed-update p50 remains
16.990–21.147 ms at 1,024 panes, above a 16.7 ms budget before application
rendering and GPU work. Physical GUI qualification is not rerun.

Raw attribution, frozen binaries and logs live outside the checkout at
`/tmp/dockspace-scale4-01a1a5fb/`. Final performance evidence belongs in
`docs/performance/`. Cargo uses one build job, nextest uses one worker and existing
targets are reused. Complete measurements do not overlap Cargo or CPU sampling
started by this session.
