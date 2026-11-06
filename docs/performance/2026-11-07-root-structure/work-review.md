# Independent review of the root-structure work counters

Conclusion: **no correctness or attribution blocker found in the three-file augmentation**. The fields describe successful body-construction outputs. They do not establish total traversal, allocation, committed mutation, or timing costs. This review independently covers root's counter addition; the original RootStructure implementation is context, not a new independent review of the author's own implementation.

Scope: `/tmp/dockspace-imgui-twofold-20261107-ea4161e/root-structure-work.patch`, its JSON receipt, the exact original temporary source, and the current integrated repository source. Read-only inspection and source/hash/string comparisons only. No repository or frozen-candidate edits, Cargo, compiler, tests, benchmark, timing, Git write, or sub-agent was used. This report is the only new written artifact.

## Exact source binding

- Original frozen architecture patch: SHA-256 `8dc1059d026310e0ef26a9ff5bcb1bca92cd8d00cff62a0bcf833a0c1404ad0b`.
- Separate counter augmentation: SHA-256 `4587fcc923b7cbbf79f8c74c1a96a2eae6dd7df52c770f991167fb160c646240`.
- The augmentation bytes match `root-structure-work.json` exactly.
- Regenerating the three unified diffs in memory from the frozen temporary source to actual integrated files reproduces the complete augmentation patch byte-for-byte. There are no unreported source changes in these three files.

| Path under `crates/dockspace/src/` | Original SHA-256 | Augmented SHA-256 |
| --- | --- | --- |
| `graph/root_structure.rs` | `3fb52047760880503ef65bb5cc76a6a0ece5024608c5488a73eef3e11b552bde` | `de30fe16e4587efe9cc181f5d1ca46b66250fa082c5b4363efa5f1c9a1215a62` |
| `drop_resolver.rs` | `1225ccb646fb9b0afdd764ab86e18b9593b8f87d7d78d3d349f1ab763e578432` | `3845cf1c9c247c79444d310969c119c53a76f5ae5ab228c260627b0d1371540e` |
| `docking_cost.rs` | `28ecb77733c55d60c85704cd89e85e24ab41fb6d851cdfc00e8cc6fb217ddaac` | `391de3001bd30c9e99936d244ddf31bce7d50e429922f026ce058dd4f43a989b` |

At the first review snapshot, all 16 original patch paths also matched their original frozen candidate hashes, except `graph/root_structure.rs`, which matched its augmented hash above. The original implementation plus counter augmentation has 18 paths. Its SHA-256 is `629921605c2a039fff69682296466401eeb348803515ad19f2502c3e2abe0545`, using the original manifest's format: ascending path order, then UTF-8 `path + NUL + candidate_sha256 + newline`. This binds the original implementation and the separate augmentation without modifying either frozen artifact. Root's subsequent one-line test-scope compile correction is separately bound below.

## Root-owned test compilation correction

Root reported that its first compilation found an unresolved `Arc` name in the test module of `workspace.rs`. The current integrated diff against the frozen candidate is exactly one line at `workspace.rs:1210`: `Arc::clone(...)` became `std::sync::Arc::clone(...)` in `selection_rebinds_semantics_without_rebuilding_membership_or_layout`. This function is inside the existing `#[cfg(test)] mod tests` at lines 1048–1049.

This is a test-scope name-resolution correction. It changes no runtime path, test assertion, counter, body construction or authority behavior. The initial compilation failure must remain recorded as a compilation failure; it is not an observed behavior-test failure. Root's evidence lives at `root-structure-verified/initial-missing-arc/` under its qualification directory. The original candidate and separate counter patch remain frozen and unchanged.

The exact Workspace file hashes are `c917dc4b11f292e38dc7d625c9e0a5237a2edeff4b938587749eb2289a7f2caf` before this correction and `15f785fee41d35cd25508d14d847f472000ccdfa4ece2012c534c92759a231f3` afterward. The 18-path integrated set including this explicit correction has SHA-256 `3b3cdd6f78a5775cb0a6bdb61a8e90cd15cdda9d15319726ebf1bca6fc22108a`, in the same path-set format. All three counter-augmentation file hashes still match the table above.

## Hook placement and count semantics

`RootStructure::build` calls `record_root_structure_completed` once at `graph/root_structure.rs:168`, after `finish_directory()` at line 166 and immediately before successful `Ok(structure)`. Missing-root and missing-node returns occur before that point. There is no hook in the traversal loop, and no success hook on a failed partial build.

`finish_directory` has already completed the identity directory and rebuilt `split_rows` from actual split kinds. Therefore the two lengths passed to the hook describe the actual finished body:

| Field | Exact meaning after a ledger reset |
| --- | --- |
| `completed_root_structures` | Number of successful `RootStructure::build` completions. The same root rebuilt twice contributes two. |
| `root_structure_rows` | Sum of the finished bodies' node-row lengths, including Tabs and Split rows. This is output cardinality, not the number of all node accesses. |
| `root_structure_split_handles` | Sum of the finished bodies' Split-row lengths. Each such row owns one `SharedSplit` Arc cloned from the actual arena payload during this build. This counts produced handles, not unique allocations, payload copies, or all Arc reference-count operations. |

The new comment at `drop_resolver.rs:1247` accurately describes successful construction outputs and excludes failed partial builds and allocator events. The field names do not claim total visits or allocations. In particular, `completed_root_structures` is an event sum, not the number of unique live/current roots.

An important boundary for later analysis: a body can finish successfully and then be discarded because a later command or final validation fails. Its successful build remains counted. The hook does not prove that the surrounding transaction committed. Conversely, a failed partial build may already have visited rows, cloned some split handles and updated placements, but adds none of these three success counters. Raw checked `compile_draft` construction does not invoke this graph-owner hook either.

Other excluded work includes child-edge iteration, postorder visits, sort comparisons, directory lookups, layout passes, output Arc cloning, additional placement maintenance, Vec capacity/reallocation, allocator calls, bytes allocated, and destruction. A zero count does not imply zero root walks; an observed row count is not a bound on all visits or memory traffic. The existing fingerprint counters likewise remain limited to their named work.

## Ledger gating and production behavior

`structural_work` remains under `#[cfg(test)]` (`drop_resolver.rs:1080`), and the new build hook is separately `#[cfg(test)]`. An ordinary non-test production build contains neither this hook nor these counter fields.

The new hook at `drop_resolver.rs:1292` uses the existing `update` path. `update` at line 1267 checks compile-time `DOCKSPACE_BENCH_NO_WORK_COUNTERS` before touching the thread-local Cell or invoking the update closure. The hook arguments are only `nodes.len()` and `split_rows.len()`, both O(1). No traversal, allocation, new snapshot, or iterator count is evaluated before that gate. No new graph walk enters the timing build through the disabled ledger. This is a source-level bounded-work conclusion, not a claim that an unoptimized function call has exactly zero machine instructions.

`Default`, `reset`, `snapshot` and the existing thread-local Cell cover the new flat fields automatically. There is no second ledger or separate state that can survive the established reset boundary. In an enabled ledger build, the existing copy/update mechanism now copies a slightly larger metrics value; those durations are not formal benchmark results and must remain excluded from performance comparisons.

## CSV and measured-work boundary

Only the ordinary ignored `docking_work` output changes. Its header at `docking_cost.rs:845` retains the previous 20 columns verbatim and in their original order, then appends the three fields in the order above. Static comparison confirms 20 to 23 columns. The row format has 23 fields: two named captures (`panes`, `views`) and 21 explicit arguments, matching the header. The appended arguments read the corresponding snapshot fields at lines 890–892.

The ordinary probe constructs and validates its fresh fixture before `structural_work::reset`, then calls the unchanged measured operation and snapshots before its post-operation validation (`docking_cost.rs:860` onward). Therefore Builder/fixture bodies and the fixture's initial projection are excluded; successful body construction during request/mutation/layout/export is included. Later validation and destruction do not affect the captured row.

The complete `docking_cost.rs` prefix before `fn docking_work()` is byte-for-byte unchanged from the frozen source. That includes the full formal `docking_cost` probe, its 18-column timing schema, fixture, request/mutation/layout/export functions, all `Instant` boundaries, complete owned export, fresh leaf work, and validation. The formal probe continues to require the compile-time disabled ledger, while the work probe requires it enabled. The augmentation does not move any operation work outside a timer or remove output work.

The unchanged comment above the work CSV originally introduced the prior appended store/copy columns. The new three columns must be described using the completion/output semantics above, rather than grouped into allocator or payload-copy totals. No code change is required for correctness; the narrow field comment already supplies the necessary boundary.

## Falsifiable source expectations and qualification

For the existing primary fresh fixtures only, each source leaf starts with two items and the edge axis is chosen opposite the parent axis. The source remains nonempty. Source inspection therefore predicts zero successful body builds for bounds-only resize, center Item Move and cross-view center Item Move, and one successful affected-root body build for the single Item edge case. For that edge case the expected row count is `2 * (panes / views) + 1`, and shared split handles equal `panes / views`. These are predictions from the concrete current path, not executed test results or invariants for arbitrary transactions. A discrepancy should prompt attribution of the actual completion path, not a reinterpretation of the counters as visits.

No extra broad framework or mirror test is needed to accept this instrumentation's source shape. Root's ongoing serial qualification remains responsible for compilation and behavior. Root reported that qualification restarted as session 27325 after the test-scope correction; this review does not assert that either qualification session passed. The frozen independent oracle remains unchanged at SHA-256 `93afdf6e8ad79d81f2efdafc4814a4bb6076fd9f535129f82ce33f9ee12ddf4e`.
