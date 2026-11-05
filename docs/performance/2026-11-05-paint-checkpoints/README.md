# Paint-checkpoint qualification evidence

These are archived observations from one local ARM64 machine, not cross-platform
performance promises. The parent [report](../2026-11-05-paint-checkpoints-results.md)
describes scope, thresholds, public API migration and evidence limits.

## Selected results

- `qualified-twenty-one/` and `qualified-twenty-two/`: the two final ABBA groups, each 864 raw
  rows, 48 samples per product/case/size. Both meet all three 1,024-pane p50 limits.
- `final-pooled-summary.csv`: all 18 pooled product/case/size rows, 96 samples each.
- `final-qualification-statistics.json`: limits, counters and frozen binary hashes.
- `baseline-fork-lto/`: the old source and old fork under matching ThinLTO and
  codegen-unit settings. `baseline-fork/` is the ordinary-release old control.
- `official-current-final/`: a separate final-source official-egui production
  ABBA control. All 864 rows preserve the same workload counters; its verification
  record recomputes all 18 summaries and verifies source/executable identities.
  It retains the exact snapshot fallback and exceeds the fork p50 limits.
  `official-final/` is an earlier observation retained as history.
- `provenance/candidate-twenty-commands.json` and build log: final Rust build.
  The final Rust binary hash is `47db8ca79dab9b4ae0b53fe19fd69cc40dc94d01efc710ee5bfdd3d7ffc5a12a`.
- `qualified-one/environment.json`: exact build provenance of the shared final
  ThinLTO C++ reference, hash `15b40045bbd19bf24f1d9f029923f43cf5ae14e5d4d34a8645864491f88b1909`.
  Its earlier Rust condition missed the resize threshold and is not final qualification.
- `reviews/`: independent static code and data reviews; runtime gates are separate.
- `qualification/`: final commands, test/lint/doc output, failure history, source
  hashes and the explicit test-only/format/cfg adjustments after timing.
- `qualification/final-summary.json`: all 22 groups passed on `31b1759`;
  `qualification/qualification-followup/` contains their completed logs.
- `SHA256SUMS`: SHA-256 hashes for every other file in this evidence directory.
  Archive-local `.gitattributes` preserves original line endings and terminal-log
  whitespace so a fresh checkout retains the recorded bytes.

`raw.csv` includes run number. Four process CSV files preserve original stdout;
`.stderr` files preserve process stderr. Summaries use nearest-rank percentiles.
Every final workload row matches both old-fork controls for all eight workload
counters. Products retain their own native layout, fonts and emitted geometry.

## Intermediate observations

`candidate-one` through `candidate-six` use ordinary release. Candidate-seven
onward use matching ThinLTO unless explicitly recorded otherwise.
`final-production-one/` is a rejected full-LTO experiment; the final delivered
production profile uses ThinLTO. Candidate-nine's dense hit keys and
candidate-thirteen's extra target HashMap were removed. Candidate-sixteen's
Galley comparison memo was replaced with exclusive final-output ownership.
The old `qualified-two/three` passed, but a later rebuild changed the executable.
Both `qualified-final-one/two` missed resize; they remain in the archive.
Center-based spatial partitioning and complete current-bound topology reuse
then produced the two final groups named above. The removed hit lookup HashMap
trial is archived in `candidate-nineteen/`.

Intermediate changes are cumulative experiments and do not independently
attribute every later fluctuation to one edit.

`dense-key-confirmation/` contains 576 rows in four/five/five/four order. Its
summary-label repair recomputed only existing raw data; `summary-recovery.json`
records that event. No measurements were discarded or rerun for that repair.

`provenance/` includes source SHA inventories and archived local helper commands.
Absolute paths in those historical commands identify the actual local run; the
supported cross-platform runner remains `scripts/compare_products.py`.
`profiles/` contains compressed macOS sample call trees and attribution metadata.
Sampling did not overlap formal timing. It includes warmup and disposal and is
not an exact timed-phase breakdown; sampled categories can overlap.

Executables are not committed. The runtime archive remains under
`/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/`. Candidate-six's historical
shared-target executable was overwritten before freezing; its source hashes,
build and raw data remain. Historical baseline build commands lack standalone
UTC bounds. Final Rust/C++ builds and timing runs retain explicit UTC boundaries.

No GPU, physical-window or heap/RSS qualification is included. The ignored
native wall-time service test is separately identified in final qualification.

## Source identity

`provenance/source-commits.json` maps the measured source to the three
implementation commits. Final relative metadata references are resolved by
byte-exact copies of `candidate-twenty-commands.json`, its build log and source
inventory at this directory root. `provenance/portable-reference-check.json`
verifies those paths; the original runtime environment files are unchanged.
`provenance/final-review-closure.json` records closure of the review's path and
pending-gate findings without rewriting the historical review receipt.
