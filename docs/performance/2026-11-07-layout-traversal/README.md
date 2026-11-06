# Layout traversal evidence

This bundle accompanies the [result](../2026-11-07-layout-traversal.md) and
[bounded plan](../../plans/2026-11-06-003-layout-traversal-state.md).

`evidence.tar.xz` contains the original regular files with relative paths, plus an
internal `archive-manifest.json` listing each file's size and SHA-256. Archive
creation verifies every decompressed member against its original bytes.
`archive-check.json` records that check; `SHA256SUMS` covers this directory's
published files except the checksum list itself. Measurement executables are
identified by hash and retained at their recorded temporary paths, not committed.

The plain source receipt, performance gates, verification receipt and independent
source/evidence reviews are also available without extracting the archive.

| Archived dataset | Purpose | Aggregate raw rows | Sample-zero complete vectors |
| --- | --- | ---: | ---: |
| `layout-traversal/against-original` | Frozen `a2bcabc` baseline vs final layout | 8,064 | 168 |
| `layout-traversal/against-checked_move` | Frozen `fa5fb1e` vs final layout | 8,064 | 168 |
| `layout-traversal/against-imgui` | Unchanged real ImGui kernel vs final layout | 8,064 | 168 |
| `resize-repro` | Shorter process history, old frozen binaries | 1,536 | 16 |
| `resize-original-history` | Original all-size resize history, old frozen binaries | 4,608 | 48 |
| `resize-setup-control/results` | Three interleaved untimed preparation modes in one executable | 1,152 | 12 |
| Total | | 31,488 | 580 |

Rows count each dataset's aggregate `raw.csv` once; original per-process CSVs are
also retained. Each dataset includes process output, environment/commands,
summaries and full vectors. The three final comparisons also retain pooled
same-execution phase attribution. Original formal timing uses 48 samples/eight
warmups, the frozen resize diagnostics use 96/16 and setup controls use 96/18.
See the result and evidence review for mode ordering and width/ordinal limitations.

Additional members preserve:

- `layout-traversal/runtime.patch`, the two-file source/test diff against
  `fa5fb1e`, and the complete 325-input final source receipt.
- The final timing build output, exact serial checks, explicit enabled-counter
  test environment, 2,024 core/four integration/nine documentation passes and
  existing Clippy style-warning limits.
- The single-executable setup control's private historical canonical/validator
  sources, original MRU predicate, common probe source, original library wrapper,
  build output and its narrower private-source receipt. These are diagnostic
  artifacts, not a runtime compatibility implementation.
- The one-off Python comparison/control/archive scripts, lane map, original
  phase analysis and independent setup-control, source and final evidence reviews.

Reproduction starts from the recorded `fa5fb1e` source and applies `runtime.patch`
in an isolated checkout. Build the final timing test executable with the exact
command/environment in `source-receipt.json`, then run the unchanged
`docking_cost::docking_cost` test using the recorded protocol settings. The
one-off comparison scripts record this machine's absolute paths; adjust those
paths to the recreated checkout and frozen executable locations. The setup
control is a separate diagnostic: temporarily register its `mod.rs` as the
test-only `resize_setup_control` module, preserve the adjacent historical modules,
and run its recorded test command. It must not be added to the production crate.

Baseline build identities and the unchanged ImGui source/compiler receipt remain
in the preceding [checked-move bundle](../2026-11-06-checked-move/). The final
qualification rechecks those binary hashes, all pinned ImGui inputs and the 49
protected user-file hashes. Historical negative results and new negative small
move/tail values remain visible. Gates describe observed p50 qualification;
they do not assert a bound on maximum latency or a unique cause for historical
resize variation.
