# Tabs insertion COW evidence review

Findings first: reject this exact candidate. Independent raw recomputation confirms only **2/18 affected center/cross p50 improvements**, below the predeclared 12/18, and **1/6 small affected improvements**, below 4/6. Whole-matrix p50 is 8 improvements / 30 regressions / 4 ties. The archive, source binding, successful early checks, unchanged work and complete-vector evidence support that decision. There is no evidence blocker to rejection and exact restoration; no further nine ordinary gates or confirmation is required for this failed early screen.

One minor publication item remains: the later `decision.json` is correct and independently hashed below, but is not listed in the currently saved `SHA256SUMS`. It is outside the original 427-input archive, rather than a missing archive member. Add its checksum if the checksum file is intended to enumerate every public evidence file. This does not alter any measurement or the rejection decision.

This review covers `docs/performance/2026-11-07-tabs-insertion-cow.md`, its evidence directory and the matching temporary frozen/verified evidence. I independently read the archive and source pairs, reconstructed statistics, checked every saved vector and raw observation, and read the captured check/build logs. I did not execute archived scripts, Cargo, tests, binaries, benchmarks, sampling or network tools, modify source/evidence, or read another current-round review.

**Exact artifacts.**

| Artifact | Independently verified SHA-256 |
| --- | --- |
| `evidence.tar.xz` | `25b7013439703cbeaf938548295506a603c955f522ae1b758f23397573c61cc2` |
| Internal `manifest.json` | `447523511e9b95c1e83ec63ecfa72460e654bfe3e64ed847dc17f5b5b238429a` |
| `archive-check.json` | `88dd064e7a486b128f6174875161bb8714dfb223626ebe55aa6d1c760acf61fc` |
| `comparisons.csv` | `8c0c4049a0ed74093d0d70c7af9243192b134233d302cd32fae735c07dd7737f` |
| `measurements.json` | `0bb6503eee21d7681a1f9d875ea6bdb1120fffb7e43c5acee8eda48e0abf3a28` |
| `decision.json`, separately added | `fd809fc736bbb17cf05ccffd256bf5253b08e874937454d75b9833065293653b` |
| Candidate source receipt | `c83225e2cb3a1d2237bcd91bf887ba0bc2b5618016471355c0f79a254050ab4f` |
| Candidate patch | `8b80e03f89d2675e84daf54a4df6f488900020fed8136b8341d298f65cd75780` |
| Predeclared rule | `476ba9d1d992b03bb5f6858de167aa63bde9f4c206a3837fdac9bdd34c2d9eeb` |
| Actual Rust control executable | `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b` |
| Candidate executable | `291bd6b54bd8a655ef041713f4fad71d0c11dbde6dc894fb828628419875e700` |
| ImGui executable | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

The 308,828-byte archive contains exactly 427 regular inputs plus its manifest: 428 unique regular members, with no absolute/traversing paths. Every manifest byte length and hash matches. All archived originals still match the corresponding temporary files, including the exact three-file source package; every existing entry in public `SHA256SUMS` verifies.

**Source/test independence and unchanged protocol.** Actual control is immutable `c7508eec3ac44138305797691244ae9c180d7680`, retained runtime `0e441c8`. All 324 formal source hashes independently match its Git blobs. The candidate's 324-entry map differs in exactly `graph/shared_storage.rs`, `operation.rs` and `graph/copy_tests.rs`; archived base/candidate/measured snapshots and the frozen patch agree with the previously reviewed exact candidate.

The normal-check map contains 323 entries; its sole exclusion relative to the formal map is `integration/product-comparison/imgui_docking_cost.cpp`. All 323 entries match the candidate. This is an intentional inventory scope, not an omitted changed Rust file.

The frozen numeric oracle, actual solver/layout, work instrumentation, probe, Cargo configuration/lockfile and C++ comparison adapter are unchanged. In particular:

- Frozen `layout/reference.rs`: `4b6ff84c9bed4b65a202e1c771b5f176b161bbb27b99dba25e4875c7b08c07c6`.
- Formal `docking_cost.rs`: `01d8438fc79095aacae935170aff3ed7eb8b5e497aa368c9966889b789adf2d3`.
- Work instrumentation `drop_resolver.rs`: `f8b093f247417b4abe2a79dc13febb0549bf214066c4bc9dc0739a72e2579a7f`.

The original 10,000 iteration cap remains; this campaign does not include the separate profile's million-cap diagnostic modification. Fresh leaf answers, all-root projection, complete owned results, common export, timer endpoints and post-timer destruction remain the original probe. The test change extends an existing copy/ownership witness with a real Open of item 99; it does not change the reference, expected common output or runner admission.

The three archived early gates are formatting, 432 targeted tests and four focused regressions, all with successful commands/logs and normal counters enabled. The extended copy witness appears as PASS in the targeted log. The separate work invocation passes one ignored work test. The optimized, counter-bypassed timing build is independently recorded successful and bound to the candidate hash. **All twelve-gate qualification is explicitly false**; none of the missing nine gates is inferred to have run.

**Complete work equality.** The 21-row, 20-column work CSV is byte-identical to retained control, SHA-256 `88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4`. I independently checked the inventory and counters, including zero fingerprint/multiset scans, one candidate/store fork per mutation, unchanged handle counts, two payload copies per mutation, and MRU/visual copies of 4 for center/cross, 2 for edge and 0 for resize. There are six completed root bodies totaling 2,926 rows. These count copied logical payload elements and root work, not allocations or capacity bytes.

**Raw observations, vectors and statistics.** Both campaigns preserve serial ABBA then BAAB with eight runs each, 48 measured observations/eight warmups per process/cell. All 16 run receipts return zero; Rust stdout records success, C++ stdout matches its raw CSV, and every stderr is empty. Per-run CSV rows reconstruct each aggregate raw file exactly.

I validated all 16,128 raw rows for product, unique sample indices, all 21 fixed pane/view/case cells, complete inventory, alternating resize widths, state digests and exact nonnegative timing identities. Every condition/group/cell has 96 observations. I independently recomputed all 2,520 saved summary statistics across five phases using the established nearest-rank rule, then all 84 complete-operation comparisons and every aggregate statistic in `measurements.json`.

All 336 complete vector files are present. Each parses as the full ordered node/item/geometry word stream, has the correct nodes/leaves/splits/roots and complete item permutation, and hashes to its sample-zero digest. All 16 copies of each of the 21 cells agree byte-for-byte across both campaigns. The 27 cell/width digest groups also agree across all raw observations. This checks actual exported vectors, not only a success flag.

| Versus actual Rust control | Improve | Regress | Tie | Reduction range |
| --- | ---: | ---: | ---: | --- |
| Complete p50 | 8 | 30 | 4 | −13.8245% to +4.9860% |
| Complete p95 | 18 | 24 | 0 | −283.5951% to +15.4743% |
| Complete maximum | 13 | 29 | 0 | −751.3198% to +85.3092% |
| Affected center/cross p50 | 2 | 14 | 2 | 2/18 improvements; requirement 12 |
| Small affected p50 | 1 | 3 | 2 | 1/6 improvements; requirement 4 |

Negative tails are retained. For example, group-one 16-pane/four-view cross p95 is 2,292 → 8,792 ns and maximum 2,917 → 24,833 ns. Both small cross medians regress: 1,959 → 2,167 ns and 2,000 → 2,125 ns. These observations need no causal explanation to establish threshold failure; they do not prove that a particular branch, allocator, compiler or capacity policy caused them.

The separate ImGui campaign has 30/42 p50 conditions at most 2×: all 14 medium and all 14 large, plus two small. Its complete p50 improve/regress/tie counts are 2/40/0, p95 6/36/0 and maximum 13/29/0. It meets that target-inventory gate but cannot override failure against the actual Rust control. No historical target count or phase-median sum is used as evidence of a retained improvement.

**Decision, restoration and report accuracy.** The predeclared rule is dated before the checks/build/comparisons and requires confirmation only for a promising initial result. Two failed initial affected thresholds make rejection final for this exact variant; skipping remaining qualification and confirmation is consistent with that rule, not an attempt to retain partially qualified code.

The published Markdown correctly states rejection, three early gates, the unrun nine gates, unchanged complete contract, all reported negatives and the limited conclusion. Every local evidence link resolves. The later decision records the same control/candidate and no retained runtime/test change. Independently hashing the current workspace confirms **all 324 formal inputs exactly restored to the retained control**, including all three candidate files and the added witness.

This closes the measured Tabs insertion COW hypothesis as rejected. It does not establish a language/ownership lower bound or reject every possible insertion-capacity design. No further test, rerun or framework is justified merely to rescue this failed predeclared screen.
