# Product cost attribution and exact audit/query improvements

Date: 2026-11-05. Base: `868e635`. Status: qualified.

## Outcome

The user authorized further investigation of the Dear ImGui gap, including
layout/algorithm changes and substantial internal refactoring. Continue on the
existing `refactor/dockspace` branch, with qualified commits and ordinary pushes.
Preserve the pre-existing CI and review files. Retain the complete product
workload and every existing input/output integrity obligation.

The user also explicitly accepts correct ownership and architecture changes.
Prefer changing unnecessary ownership transfers over preserving an inefficient
private representation, while checking complete behavioral equivalence.

## Attribution

- Freeze the previously qualified egui and ImGui executables and verify their
  exact hashes before running a fresh 24-sample, six-warmup ABBA baseline.
- Run CPU sampling separately. The current first-eight-second 1,024-pane sample
  chiefly covers idle and readiness. Audit-related call trees cover 5,255 of
  6,461 main-thread samples (81.33%), while tessellation covers 90 (1.39%).
  Product painting includes the audits and cannot be added to them. Standalone
  layout/compiler frames can be inlined or absent on cached idle work; do not
  turn a zero sampled symbol count into a zero layout-time assertion.
- Add a standalone Rust example scenario selector outside timed execution so
  changed-layout and changed-input profiling can be isolated. The comparison
  runner continues to run all three cases and verify complete visible output.
- Inspect the layout solver, measurement and scene rebuild paths. Distinguish
  cached idle behavior from resize/click rebuilds and identify nested searches
  with concrete current-plan membership requirements.

## Selected work

1. Preserve the exact append-check predicate while comparing borrowed live
   paint lists at callback completion. Avoid building and dropping another
   complete owned snapshot for a Boolean result. Preserve unknown-layer
   rejection by temporarily detaching only existing known lists, checking a
   cloned residual and restoring the originals without changing missing/empty
   entries, ShapeIdx, order or transforms. Owned output proofs retain their
   existing snapshots and terminal checks.
2. Remove nested linear junction/arm searches from axis operability queries.
   Prove canonical sorting before binary search, retain exact current-plan
   membership and all policy checks, and handle the bounded two-arm collection
   without collecting/sorting a temporary vector.
3. Evaluate the repeated value comparison of immutable text galleys. A cache may
   be local to one paint call, hold strong Arc owners and memoize exact
   self-equality, including non-reflexive NaN and upstream debug-panic behavior.
   Same-owner comparisons
   can then reuse that result; distinct owners must retain value equality.
   Do not use a hash or pointer alone as output proof or keep a long-lived cache.
4. Evaluate reusing owned snapshot buffers across callbacks without trusting a
   closed rendering interval. A new begin must still read every current live
   descriptor under the graphics lock. Only complete, non-panicking descriptor
   identity can permit reuse; unsupported or changed values must be cloned from
   the live list. Cache only pure supported values between callbacks and release
   opaque callback owners at the original check boundary. Preserve fresh layer
   metadata, missing/empty state and unknown-layer checks. This is a measured
   ownership experiment, not permission to infer that prior output stayed intact.

Unrestricted egui Ui/Context access permits arbitrary edits to old PaintLists,
and the public version exposes no mutation revision. Exact per-callback checks
therefore still require inspecting accumulated prefixes. Whole-graphics staging
can protect prior output but changes ShapeIdx/read behavior and rejection of
invalid callbacks; it is not a transparent replacement for the current contract.

## Verification and completion

- [x] Independently review concrete candidate changes and differential tests.
- [x] Run focused append-proof and current-plan junction regressions serially.
- [x] Compare complete product work and raw diagnostics with the fresh baseline;
  keep every process output, raw row, percentile, compiler and source/binary hash.
- [x] Sample the reduced implementation on idle and changed-layout cases and
  report what remains expensive without treating sample counts as exact clocks.
- [x] Run required workspace, host, official egui, product and native consumer
  gates with one Cargo job and one nextest worker; include Clippy, strict docs,
  doctests, format and dependency checks. No builds/tests/sampler during formal
  timing. Reuse existing targets.
- [x] Record final results and limitations and prepare intended paths for the
  authorized branch publication. Leave pre-existing CI/review work untouched.

## Qualified result

The [complete result](../performance/2026-11-05-product-cost-attribution-results.md)
retains all five conditions, eight separate profiles, frozen source inventories,
raw outputs and a 576-row interleaved version confirmation. Final 1,024-pane p50
is 31.025 ms idle, 83.109 ms per two-frame click and 43.177 ms per resize, a
51.55–59.04% reduction from the fresh baseline. All work-count diagnostics are
unchanged. ImGui remains 1.098/2.202/1.105 ms, and small-layout tail regressions
are explicit.

Fifty-one focused tests, all 18 serial gates and independent crossed reviews
pass. Review also closed a duplicate-LayerId prefix-truncation defect from the
previous move-extraction implementation; malformed duplicate rosters now make
capture unavailable. The implementation retains complete live descriptor
observation and exact after checks. It does not establish linear auditing,
unchanged application Arc counts, a heap/RSS reduction or physical GPU latency.
