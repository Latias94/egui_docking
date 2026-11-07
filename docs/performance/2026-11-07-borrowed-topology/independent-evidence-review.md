# Borrowed topology evidence review

**Accept the rejection. No evidence blocker found.** The complete initial comparison fails both predeclared Rust-control improvement gates: 26/42 complete p50 improvements against the required 28/42, and 3/14 small-layout improvements against the required 8/14. The separate 30/42 ImGui inventory does not override those failures. An additive confirmation or the remaining nine ordinary gates is not warranted for this failed screen.

This independently closes the specific borrowed-topology follow-up requested in my previous continuation challenge. It is not a new overall stopping judgment, and it does not assess the separate scalar-issuance candidate now in progress.

## Scope and source binding

Reviewed directory: `docs/performance/2026-11-07-borrowed-topology/`, including the newly completed sibling report `2026-11-07-borrowed-topology.md`.

The actual control is `c7508eec3ac44138305797691244ae9c180d7680`, retained runtime `0e441c8`, executable `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`. The measured candidate is `73188fa44e168aaec6f591f029086b04dbc414ab0c4b79fcc21d44f4b0369d49`.

I independently checked the following source bindings:

- Both frozen source receipts contain the same 324 paths. Their only changed hash is `crates/dockspace/src/layout.rs`.
- All 324 control-receipt hashes match the corresponding immutable `c7508ee` Git blobs.
- The archived actual-control and measured-candidate layout files match their respective frozen hashes and the exact source-package pair. The archived runtime patch matches the previously reviewed patch.
- The normal-check receipt covers 323 Rust/workspace inputs; every hash matches the candidate's frozen receipt. The additional formal input is the pinned `integration/product-comparison/imgui_docking_cost.cpp`. This difference in receipt scope is not source drift.
- Both campaigns bind the same candidate source receipt and executable. Reading the existing frozen executable files also reproduces the recorded control, candidate and ImGui hashes.

The changed layout file is exactly the borrowing treatment previously reviewed: direct projection borrows the workspace's topology; the private helper accepts `&Arc<RootStructure>`; delayed and raw inputs retain their owners; both result maps still acquire owned topology references. No numerical executor, callback, output field, test, timer or exporter change is present.

## Qualification and counter boundary

This is **three-gate early screening, not twelve-gate qualification**:

| Recorded check | Result |
| --- | --- |
| Formatting | Return code 0 |
| Targeted normal-counter nextest | 432 passed |
| Focused regression nextest | 4 passed |
| Separate normal-counter `docking_work` | 1 passed; complete 21-row, 20-column CSV byte-equal to retained control |

The normal receipt records counters enabled and unchanged source hashes. The separate production-profile test build records `DOCKSPACE_BENCH_NO_WORK_COUNTERS=1`, success, unchanged sources, and the candidate executable hash above. The timed probe is therefore separated from the normal structural-work ledger; the unchanged work CSV is a logical-work check, not an allocator measurement or a measurement of atomic reference counts.

The root report and decision accurately state that the remaining nine ordinary gates were not run. They do not claim twelve-gate qualification for this rejected candidate.

## Independent raw/statistical checks

I did not execute any benchmark or archived script. Static parsing of the saved CSVs reproduces the measurements:

- Each comparison contains two serial groups in ABBA/BAAB order, eight successful processes, the full 21-cell matrix per process, 48 recorded samples per cell, and eight declared warmups.
- Each process contains exactly sample indices 0 through 47 in every expected cell. Each campaign has 8064 raw rows and 96 observations per condition/group/cell.
- The complete archive therefore contains 16128 raw observations. Both raw files exactly reconstruct their per-run CSVs with only group/run/condition metadata added.
- All p50, nearest-rank p95 and maximum values for all five phases in both summary files reproduce from the raw observations. No phase medians were added to manufacture a complete-operation result.
- All 84 comparison rows reproduce from the corresponding summaries, including ratios and percentage reductions.

Against the retained Rust control:

| Complete metric | Improve / regress / tie | Reduction range |
| --- | --- | --- |
| p50 | 26 / 16 / 0 | -12.7681% to +7.4373% |
| p95 | 23 / 17 / 2 | -65.3438% to +10.6591% |
| Maximum | 19 / 23 / 0 | -1292.1022% to +67.9514% |

The small-layout phase gate is independently 3/14. Candidate/ImGui p50 is within 2x in 30/42 conditions, and every medium/large condition passes that separate threshold.

The worst complete p50 and p95 rows are group 2, 16 panes, four views, cross-view move: 1958 to 2208 ns p50 and 2167 to 3583 ns p95. The same row's maximum is 2583 to 35958 ns. These observations remain in the full table. One campaign does not establish their repeatability or a compiler cause; the already failed primary gates make either claim unnecessary for rejection.

## Complete outputs and archive

The 336 saved state vectors cover every expected process/cell in both comparisons. For each of the 21 logical cells, all sixteen saved vectors are byte-identical across products, candidates, groups and runs. I independently recomputed each vector's word-wise digest and matched it to that process's sample-zero raw row. Raw digest equality also holds across observations sharing cell and width. This verifies the saved complete vectors and digest records; it does not pretend that every raw observation has a separately saved full vector.

All seven entries in the outer `SHA256SUMS` match their files. The archive contains 418 regular input members plus `manifest.json`, hence 419 regular tar members in total. Every input matches its manifest byte length and SHA-256; there are no duplicate member names or unsafe paths. Every archived input also matches its corresponding preserved source-package, frozen-result or verification file. The archived manifest and counts agree with `archive-check.json`.

These checks support the stated archive round trip. The archive preserves the exact changed source pair and full source-hash receipts; the unchanged control content is recoverable from the identified immutable commit. “418 inputs” should not be interpreted as 418 complete runtime source files.

## Restoration and decision boundary

The decision is timestamped `2026-11-07T13:30:39.805620+00:00` and records rejection and restoration of all 324 inputs. The experiment ledger records the same historical event. I separately read `layout.rs` during this review and confirmed its retained hash, `01f21abad6c4d71856dbf17ae9eec90a97d7fbe7717bb2e6da87895f2fed5dcd`.

The all-324 restoration statement is **point-in-time evidence**, not a claim that the live worktree will remain identical while Root prepares the independent next candidate. Subsequent authority/workspace/test changes are outside this experiment and do not invalidate its restoration. If the sibling report's opening “All 324 formal inputs match” is quoted during that later work, retain this temporal qualification.

The root report otherwise agrees with the evidence, preserves the negative tails, distinguishes early screening, and correctly declines to assign an allocation share or compiler cause. No confirmation should replace this failed initial result. This rejects the private borrowed-topology implementation; it does not prove that Arc operations are free, all borrowing optimizations lose, the remaining target is impossible, or the runtime is optimal.

## Artifact bindings

| Artifact | SHA-256 |
| --- | --- |
| `evidence.tar.xz` | `aa515f29512b5c801216e714f8c31ef2b1f729be470fae10af40e24f7ee41d1d` |
| `comparisons.csv` | `905d8c38a805303ab216aad4ff268807edcb4106176740974d3faed2987e650a` |
| `measurements.json` | `70d89f40f698487661c5c19bd6a9a88d006db956fa3b3a2e14d7e496e1e651f5` |
| `archive-check.json` | `667fd6208ffa5eb119aee96bfcd91ed1fac3bb77399540cc709d94699b90dbbc` |
| `decision.json` | `13d6bcab06d6bf768a532090dc68b09ce477ecb47ce30725a73696bcfdfe4f61` |
| `implementation-review.md` | `31825db69b8e96c5b9a1dfa3474b2a15a19d25bd5cc0391375d09ad72e7a319b` |
| `source-review.md` | `7a56b163a449fe84e0e0ee1b8e813b1e8cd825b8cb1295191aad37616133b167` |
| `SHA256SUMS` | `b3d65ee0f4169b6e8434eb403bc0c7767a58acf83f5ace361ec4cf33c97a6c05` |

Only this review file was written. No Cargo, build, test, runner, timing, sampling or network command was executed; no repository, candidate, fork or cache content was modified.
