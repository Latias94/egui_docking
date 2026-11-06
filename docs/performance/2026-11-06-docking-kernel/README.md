# Docking-kernel evidence

This archive supports [the results](../2026-11-06-docking-kernel-results.md)
and [the executable protocol](../../../integration/product-comparison/docking.md).
Runtime base: `a9ace585e1e8d812efd35c6149f549043bb7c999`. The formal environment
records hashes of the actual modified source, not just that base commit label.

- `formal/environment.json`: exact source, pinned upstream and executable
  hashes; compiler settings; commands; serial build and run intervals.
- `formal/raw.csv.gz`: all 8,064 measured operations, with five directly
  recorded durations and complete-state digests. No tail samples are removed.
- `formal/summary.csv`: 84 nearest-rank group summaries, 96 samples each.
- `formal/pooled-attribution.csv`: 42 product/cell summaries, 192 samples each,
  with p50/p95/max and additive phase sums/shares from the same executions.
- `formal/group-*`: every process's original raw CSV/stdout/stderr and exact
  21-file sample-zero state set. State dumps, logs, stdout and large CSVs are
  gzip-compressed. There
  are 168 complete vectors; all corresponding vectors match. Full vectors for
  the second resize width are checked by the running probes but are not dumped;
  every measured sample retains its cross-product digest.
- `formal/build-*.log.gz`: build receipts. The formal Rust build reused a valid
  production artifact and froze a copy before timing. Normal counter/test
  builds later reuse the target directory without changing the frozen copy.
- `work-counts.csv` and `.log.gz`: independent ordinary build, same driver,
  counters reset after setup and read before external checks. These are work
  counts, not timing results.
- `transaction-source-attribution.md`, `imgui-source-attribution.md`: source
  paths, required contracts, implementation choices and predictions.
- `profile-center/`: separate eight-second CPU capture of the frozen Rust
  executable, 1,024 panes/one view/center move, plus launch settings and output.
  Fixture work, external checks and disposal are included in the sampled
  process. Its timings are not formal results, and flat symbol percentages
  cannot be used as operation phase shares.
- `implementation-review.md`, `formal-evidence-review.md`,
  `root-cause-review.md`: independent review and closure.
- `checks.json` and `checks/`: root-run verification receipts/logs.
- `SHA256SUMS`: checksums of every other archived file, over the stored bytes.

Gzip files use deterministic headers. Environment result hashes refer to the
original uncompressed CSV bytes, so decompress them when checking those
hashes. Executable hashes are retained but binaries are not committed; local
copies remain under
`/tmp/dockspace-docking-kernel-20261106-a9ace58/formal/`.

The formal comparison is actual structural commands, all-root layout and the
same complete fresh owned state export. It measures logical roots in one
context, not native OS viewport lifecycle or rendered UI finalization. Rust's
stronger checked transaction and richer internal layout results remain in the
timed boundary. No production runtime optimization is claimed by this archive.
