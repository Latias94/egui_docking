# Source-nonempty evidence review

Review date: November 7, 2026. Reviewer: root_frontier_placement. This review is based on read-only source, immutable Git objects, frozen receipts, and independent parsing of the saved observations. No build, test, benchmark, profile, archived script, or network request was executed. Only this report was written.

Recommendation: **reject this candidate and keep the previous control**. I found no source-contract or evidence-integrity blocker. The performance retention blocker is the unresolved regression in unaffected resize workloads after the one permitted confirmation campaign. Both move gates pass twice, and the ImGui inventory gate passes, but these are not sufficient to waive the separately predeclared resize guard.

The two 1024-pane resize cells in group 1 regress in both Rust campaigns: approximately 3.20% then 2.18% for one view, and 3.81% then 3.70% for four views. The confirmation also has a pronounced group effect: all six group-1 resize cells regress, while group 2 has five improvements and one tie. This limits causal interpretation; it does not establish a compiler explanation. It also leaves insufficient evidence for safely retaining this binary as the next control. No further measurement is requested.

## Reviewed snapshot and binding

The actual control is immutable `c7508eec3ac44138305797691244ae9c180d7680`, whose retained runtime is `0e441c8`. The candidate changes only `crates/dockspace/src/operation.rs`: four hunks, +19/-8. I reconstructed the candidate from the saved full base and unified patch and checked exact equality with the source package, frozen patch, and measured-candidate source.

| Bound artifact | SHA-256 |
|---|---|
| Actual-control operation.rs | `7586ec06b29b77a794133e0c554d4fcca7a6faa16b8c550b86034ad394175862` |
| Final candidate operation.rs | `bf81e49acdf0678cbb25cdf1f769fa9e465a45a9034dd349342398f839eee349` |
| runtime.patch | `a67c0b88a9f0a9e6c77ae8c5e1084c72f3ac958fd454211f21121d65cb95a2db` |
| Predeclared rule | `92c9ee1a5645800d68688d6cf671709d0ac43f6031cd003723b0c83b6638edd9` |
| Candidate formal source receipt | `e17b329316fb30c80f14e7140f6f7b8e10af046301b8b9326699bac95ea362c2` |
| Final checks-all.json | `6292797c47acac51946b0579b405378dabe90390e71348bdfb7b212a918a494a` |
| Frozen candidate executable | `0ac7566065a4a7b23b88b2c7f0c690122d0ae6cd4e7227693f85f29b73e48317` |
| Frozen Rust control executable | `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b` |
| Frozen ImGui executable | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

All 324 control-receipt paths match immutable c7508ee by content. The candidate formal receipt has the same paths, with only operation.rs changed. All 323 qualification-receipt paths match the final formal candidate; the additional formal path is the unchanged C++ comparator. Executable hashes were checked from the existing frozen files and agree with the campaign records. No current working-tree candidate was substituted for these bindings.

The final formal build is opt-level 3, debuginfo 0, debug assertions off, overflow checks off, with `DOCKSPACE_BENCH_NO_WORK_COUNTERS=1` and one build job. It is the existing production test artifact, with the original timed boundary. An existing presentation_identity_frontier dead-code warning remains; this is not a warning-free claim.

The source change returns the current remaining-items result from the successful remove_item operation, after the original item/MRU removal and selected repair. Both private callers pass it to immediate source-root cleanup. The close caller's intervening successor selection does not remove another item. Cleanup retains its original root lookup and error, central-root behavior, and whole-root fallback when the source became empty. A nonempty source Tabs node already establishes that the root still contains an item. No global count, new authority, persistent proof, weaker raw validator, or new ownership mechanism is introduced. MRU, rollback, old references, independent candidate construction, and complete owned output remain covered by the unchanged paths. The more detailed source review is preserved as source-review.md.

## Qualification, history, and logical work

The preserved pre-format receipt contains a real failed `fmt --check`, starting at 10:30:27 UTC. It ran no other gate. Its operation.rs hash was `d1094f40e2f05f158f6eefb0f98e7c918d79342b0812743a2ea320afed10aae1`; its other 322 qualification paths match the final version. The signature was formatted, and all twelve gates then ran afresh against the final bf81 source. The historical failure is not counted as a successful gate.

The final checks ran serially from 10:31:48 to 10:49:02 UTC, with normal work counters enabled and source hashes unchanged. The saved commands, return codes, PASS records, and log summaries agree:

| Test gate | Passed | Skipped |
|---|---:|---:|
| Targeted | 432 | 1644 |
| Fixes | 4 | 2072 |
| Production core | 2073 | 3 |
| Public integrations | 4 | 0 |
| Workspace | 2249 | 3 |
| Host default | 292 | 0 |
| Host serde | 335 | 0 |

The other five gates are formatting, Clippy, strict documentation, doctests, and dependency checks, all returning zero. Clippy denies correctness and suspicious diagnostics while allowing other warnings; documentation uses `RUSTDOCFLAGS=-D warnings`. Doctest summaries report 9, 0, and 1 passed, with no failures.

The subsequent ordinary docking_work run reports one passed and 2075 skipped. Its 21-by-20 work CSV is byte-identical to the retained projection-stack-reserve control ledger, SHA-256 `88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4`. It preserves six successful bodies and 2926 root-structure rows. This ledger covers logical successful graph-row and payload work; it does not count allocator calls, traversal capacities, or all work avoided by the new branch. No new mirrored boolean test was required for this review.

## Archive and observation audit

The reviewed evidence.tar.xz is 497332 bytes, SHA-256 `2b2b8ff022ec8b31acc128dedc6bc33ab3e0560eec7685931b39cb1cf3af2e68`. It contains 624 unique safe regular members: 623 inputs plus manifest.json. The manifest hash is `626aacd662174251c0beef549295bfbcc040c2435777df420ca192b8b112466e`.

All 623 input lengths and hashes match, and every member agrees with its original input. The inputs comprise 617 execution artifacts, four source-package files, one actual-control source, and one measured-candidate source. All eight SHA256SUMS entries match. The other concurrent review's contract-review.md was hashed for archive integrity only; its conclusions were not read or used.

There are three separate full campaigns:

| Campaign | UTC interval | Raw observations | Complete vectors |
|---|---|---:|---:|
| Initial Rust control pairing | 10:52:52.386249–10:52:56.591073 | 8064 | 168 |
| Formal ImGui pairing | 10:52:56.630595–10:53:33.747535 | 8064 | 168 |
| Additional Rust confirmation | 10:55:55.132769–10:55:59.183647 | 8064 | 168 |
| Total | Three separately retained campaigns | **24192** | **504** |

Each campaign has two full groups, ABBA then BAAB, eight processes, 21 workloads per process, 48 samples, and eight configured warmups. Each process contributes 1008 observations. All 24 saved process intervals are serial, return codes are zero, stderr is empty, Rust stdout records the harness pass, and the ImGui stdout agrees with its CSV. Per-process CSVs concatenate exactly to their aggregates.

I independently checked sample ordinals, cardinalities, phase sums, alternating resize widths, and all 252 summary rows across the five phases. Each summary uses 96 observations, with nearest-rank indices 47, 91, and 95 for p50, p95, and max. All 84 initial comparison rows and all 42 confirmation rows match independently recomputed absolute values, ratios where present, and percentage reductions. Campaigns were neither pooled nor substituted for one another.

All 504 complete vectors were parsed, including root order, DFS child paths and generations, four-unit gaps, selected tabs, and each item's exactly-once presence. For each workload the full exported vector agrees across products, runs, and campaigns. The little-endian-u64 FNV digest matches the exported sample-zero state. All 27 workload/width digest keys agree across all 24192 observations. Complete-vector files are sample-zero dumps; this does not claim that a full state dump exists for every timed sample.

The clock still includes capture, checked mutation, fresh projection of every root, and independent complete owned export. The unchanged comparator and output checks remain part of the protocol.

## Complete results and negative observations

Counts below are improvements / regressions / ties. Positive reduction means lower candidate latency. Rust comparisons use the actual retained control; the ImGui table is a separate comparator, not another Rust replication.

| Campaign and subset | p50 | p95 | max |
|---|---:|---:|---:|
| Initial Rust, all 42 | 22 / 19 / 1 | 19 / 22 / 1 | 22 / 20 / 0 |
| Initial Rust, 30 moves | 21 / 9 / 0 | 16 / 14 / 0 | 16 / 14 / 0 |
| Initial Rust, 10 small moves | 10 / 0 / 0 | 6 / 4 / 0 | 6 / 4 / 0 |
| Initial Rust, 12 resize cells | 1 / 10 / 1 | 3 / 8 / 1 | 6 / 6 / 0 |
| Confirmation Rust, all 42 | 26 / 14 / 2 | 26 / 14 / 2 | 24 / 15 / 3 |
| Confirmation Rust, 30 moves | 21 / 8 / 1 | 22 / 7 / 1 | 18 / 10 / 2 |
| Confirmation Rust, 10 small moves | 9 / 1 / 0 | 7 / 2 / 1 | 4 / 5 / 1 |
| Confirmation Rust, 12 resize cells | 5 / 6 / 1 | 4 / 7 / 1 | 6 / 5 / 1 |
| ImGui pairing, all 42 | 2 / 40 / 0 | 8 / 34 / 0 | 19 / 23 / 0 |

| Campaign, all complete cells | p50 reduction range | p95 reduction range | max reduction range |
|---|---:|---:|---:|
| Initial Rust | -14.224507% to +23.614142% | -36.720000% to +64.000000% | -1088.820302% to +83.404453% |
| Confirmation Rust | -6.176325% to +19.259688% | -108.784818% to +63.697778% | -610.986324% to +92.300862% |
| ImGui | -217.432150% to +15.972755% | -304.200000% to +19.685716% | -1422.991071% to +83.555123% |

Initial small-move p50 reductions range from +1.366667% to +23.614142%; confirmation small moves range from -5.857445% to +19.259688%. Initial move p50 reductions range from -3.679787% to +23.614142%, and confirmation moves from -6.176325% to +19.259688%. The favorable small-move evidence is therefore real within this saved protocol, but does not imply that every move or tail improves.

Against ImGui, the candidate is at or below 2x in 30 of 42 cells: 2/14 small, 14/14 medium, and 14/14 large. Candidate/comparator p50 ratios range from 1.932800x to 3.174322x for small, 1.237632x to 1.533382x for medium, and 0.840272x to 1.135008x for large. The twelve misses remain small workloads; passing the inventory gate does not establish an improvement over the retained Rust binary.

Several adverse tails are substantial and remain in the retained data. The initial group-1 16-pane/one-view resize is 1167→1333 ns at p50 and 1250→1709 ns at p95. The initial group-2 16-pane/four-view center move has a max of 2916→34666 ns. In confirmation, the group-2 128-pane/one-view edge move is 14167→15042 ns at p50; group-2 1024-pane/four-view cross-view move is 88209→184167 ns at p95; group-1 128-pane/four-view resize has a max of 11041→78500 ns. These are disclosed observations, not proven allocator or compiler causes. Max-only outliers do not by themselves establish a repeatable mechanism.

The resize comparison that determines the retention recommendation is:

| Group | Panes | Views | Initial complete p50 reduction | Confirmation complete p50 reduction |
|---:|---:|---:|---:|---:|
| 1 | 16 | 1 | -14.224507% | -0.082781% |
| 1 | 16 | 4 | 0.000000% | -3.391232% |
| 1 | 128 | 1 | -3.259656% | -2.001930% |
| 1 | 128 | 4 | -0.877471% | -0.480000% |
| 1 | 1024 | 1 | -3.196729% | -2.184662% |
| 1 | 1024 | 4 | -3.809827% | -3.704888% |
| 2 | 16 | 1 | -3.476821% | 0.000000% |
| 2 | 16 | 4 | -10.000000% | +11.723164% |
| 2 | 128 | 1 | -4.858441% | +1.952941% |
| 2 | 128 | 4 | -1.333333% | +2.354806% |
| 2 | 1024 | 1 | -2.992746% | +0.297854% |
| 2 | 1024 | 4 | +1.054243% | +0.795451% |

Of the six confirmation resize regressions, **five**, not six, were also negative in the same initial cell. The sixth was initially tied. The group-1 16-pane/one-view confirmation loss is just 1208→1209 ns and is not a material-regression argument. The large initial small-cell losses did not persist: the initial 14.22% loss shrank to one nanosecond, and the group-2 10% loss reversed to an 11.72% improvement.

The group-1 large resize cells are stronger reasons for caution: one view is 71667→73958 ns initially and 70583→72125 ns in confirmation; four views is 72208→74959 ns initially and 71959→74625 ns in confirmation. Those same-cell losses recur while group 2 changes direction for several other resize cells. Across both groups and both Rust campaigns, the 1024-pane/one-view resize is negative in three of four p50 comparisons and all four p95 comparisons. The four p95 reductions are -12.402290%, -3.010592%, -3.808567%, and -0.816082%. This statement concerns the one-view workload, not all large resize tails: group-1 four-view p95 improves in confirmation. Neither discarding group 1 nor averaging it away with group 2 would respect the original rule.

## Retention judgment and limits

The predeclared file was saved at 10:29:32.557107 UTC, before implementation qualification and all timing. It requires at least 20/30 affected move cells and 6/10 small move cells to improve, at least 30/42 cells and all medium/large cells to meet the ImGui threshold, preservation of semantic gates and logical work, disclosure of all tails, and rejection of repeatable material resize regressions without selective campaign use.

Both Rust campaigns satisfy the move gates independently, and the sole ImGui campaign satisfies its gate. The resize guard remains adverse after the allowed confirmation. The rule did not supply a numerical definition of materiality, so this review does not introduce a retrospective percentage threshold. The repeated losses in two large unaffected workloads, together with unresolved order/group sensitivity and the saved adverse tails, justify conservative rejection rather than declaring safe retention. This is a retention judgment on the full binary evidence, not a claim of statistical significance or an exclusive explanation for the losses.

The source edit is not on the resize execution path. That fact is useful for attribution, but cannot exempt measured whole-binary resize behavior from a predeclared guard. Source deletion of a traversal and its allocations also cannot independently establish a complete-latency win.

Keep the initial campaign, the additional Rust confirmation, and the original ImGui campaign separately available. Do not replace the initial numbers, pool campaigns to select a favorable result, change the predeclared gates, or run another confirmation for this decision. The next candidate should bind to the previous retained control unless a separately authorized central decision says otherwise.

During finalization, Root recorded the central `reject_restore` decision at 11:05:17.569945 UTC. I checked its control identity, candidate hash, statistics, and restoration fields against the evidence above, without reading the other reviewer's conclusion. I then independently rehashed all 324 current formal source paths against the already immutable-bound control receipt: zero missing paths and zero mismatches. Thus the source and test set is restored exactly to the previous control; no candidate runtime or test delta remains in that set. The separately written decision.json is not one of the 623 archived inputs or the original eight SHA256SUMS entries. The archive remains byte-identical. No publication commit or top-level narrative existed at the last file check, so this report does not certify either.

## Durable evidence hashes

| Artifact | SHA-256 |
|---|---|
| Initial comparisons.csv, 84 rows | `58eb01e94a459a840480dfc7cde50ce1494b171f70bf98215b27555a5df415a5` |
| Initial measurements.json | `9c36d3711e124c32fb873b5282ec2f3ece74bd62b483f0e0febdb98545005aba` |
| Confirmation comparisons, 42 rows | `68047b8beb5c091c15b7686f0a12aeeeea85b411ab53fa77f4b7199338b19ecf` |
| Confirmation measurements.json | `b6acdb141fde29e6c849613e88eef4c5c7387ef9183e2519be0077538450b1aa` |
| Initial Rust raw.csv | `8815c7e2fbab138f0f219761fbb36a551704341a72106e15cf7ef4aa20ade8eb` |
| Initial Rust summary.csv | `f761acef966276dd39808d2a5bf8e18becfb7a7d1535aac4c9470d3419356572` |
| ImGui raw.csv | `0efd5ffe6fd88833ba8908c5d3ac8a1a5aad6a1a5126da82fbfd783253e19db6` |
| ImGui summary.csv | `97f84a4fe0d89a01832a61956ca4ede4b93bc8b198abe1591f3d947d8d0941d6` |
| Confirmation raw.csv | `2cb23a02962b77df18408410da2c0b759a00901c263d52a56440ca2a2c918d1d` |
| Confirmation summary.csv | `e1e238cc58526cb0a3c698a7cea5f11df54892b7c29fb3761dbb1a2122351f1f` |
| Archive round-trip check | `20d4500ef17ef64dbe94aeeafb8a5a2257c263ce517a57cd72944cb774c40424` |
| Source review | `5d3f0df155644a327fe473ded924bf5f71f8f631a3535ce9673951d85253dbda` |
| Other contract review, hash only | `63c9eca10de627e0a223e1b0d8aef148e55086239714c7bb11c6e137ade1def8` |
| SHA256SUMS | `b94b5ced04498ee0ac1ede6847db8f461085d998180cc39e2034e1507e7f379a` |
| Later central decision.json | `8de3bffbacfcd3b73ebc2c56fb5d091e12b3e016b0474d3e0ef3ba63ae9333ba` |

No source or evidence-integrity blocker was found. The blocker to keeping this candidate is the unresolved repeated unaffected-resize regression under its own predeclared guard. The favorable move results remain valid evidence for this rejected attempt.
