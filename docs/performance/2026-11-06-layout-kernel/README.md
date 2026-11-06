# Layout-kernel evidence

The [report](../2026-11-06-layout-kernel-results.md) explains the measured
boundaries and interpretation limits.

- `final/`: the two final ABBA groups, all 3,456 raw samples, group and pooled
  summaries, per-process output, source/executable hashes and build logs.
- `final/verification.json`: independently recomputed statistics, complete
  integer-geometry digests, source identities and serial timing checks.
- `final/checks.json`: scoped nextest, Clippy, formatting, Python and diff checks.
- `initial-probe/`: the first two-group observation, retained separately. Later
  runner changes add source-provenance guards; Rust/C++ boundaries are unchanged.
- `SHA256SUMS`: every other archived file, preserved byte for byte.

The Rust executable is an optimized test build invoking private existing layout
functions. No renderer runs in either timed path. Rust constructs a richer
immutable projection and ImGui mutates existing bounds; this is a subsystem
comparison over matching geometry, not general feature parity.

Each group's 192 samples per case pool two processes with 96 samples each;
pooled summaries contain 384 samples per case. Prepared Rust input measurement
remains separately recorded in `preparation_ns`; complete projection includes it.
Every node is validated after every operation. The Python verification also
reconstructs the expected geometry digests for all six size/width combinations.

`source_commit` in runtime metadata identifies the base before adding the probe.
The source inventory identifies the actual measured inputs, including the new
ignored test module and comparison runner. The core layout implementation itself
is unchanged. Runtime executable paths are historical local paths; executables
are not committed.
