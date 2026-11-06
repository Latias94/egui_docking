# Architecture cost diagnosis evidence

See the [diagnosis](../2026-11-06-architecture-root-causes.md) for mechanisms,
necessary semantics, the measured manifest pilot and the proposed refactor.

- `baseline/`: existing complete public headless workload on `0a1dcf8`.
- `candidate-manifest/`: the exact same workload with only the frozen manifest
  inventory shared; popup changes copy on write. Build logs are gzip-compressed.
- `allocation-comparison.json`: matched p50 timing/allocation/byte observations
  from those two separately instrumented runs. `requested_bytes` sums allocation
  requests and is neither RSS nor the current live heap.
- `frozen-timing/`: two timing-only ABBA groups of the frozen baseline/candidate
  binaries, all scales, 48 samples and six warmups per process. Each process
  CSV preserves its own p50/p95/max. No medians are pooled.
- `manifest-red.*`: the regression initially failed because cloned manifests
  deep-copied frozen rows. `source.diff` is the final single-file change.
- `checks.json` and `checks/`: green regression, full workspace, Clippy, format,
  default/serde host, native and diff check records.
- `verification.json`: coverage, matched workload/output-state columns, source
  differences, unchanged workload, executable hashes and protected-user checks.
- `helpers/`: local orchestration scripts, archived as historical commands.
  The supported public workload is the existing `frame_cost` example.
- `independent-review.md`: read-only code/data review receipt.
- `SHA256SUMS`: hashes for every other file in this directory.

These are CPU diagnostic measurements with the headless recording sink included.
They do not establish pure-core allocation totals or ImGui feature equivalence.
The existing workload emits summaries rather than individual samples. Every
summary is retained, but an independent p50/p95 recomputation is not possible
without the unretained individual timing values. Allocation instrumentation and
CPU timing are separate conditions; each allocation count is a measured summary.

No font, epaint, egui, ImGui, GPU or native-window work is part of these runs.
The measurement-change case changes tab intrinsic width as well as bounds.
The selection case is a semantic command, distinct from a two-frame real click.
The in-flight case retains one older output. All scenarios encode the complete
ordered paint program and settle actual core output receipts.

The production Rust source difference is confined to frozen-inventory ownership
in `scene_manifest.rs`; every original workload source remains unchanged.
Runtime binaries and the full build logs remain under
`/tmp/dockspace-architecture-20261106-0a1dcf8/`; binaries are not committed.
