# Placement-roster-borrow early-screen evidence review

November 7, 2026. Independent read-only audit by root_frontier_placement. Only this report was written. No Cargo command, test, benchmark, profile, runner, candidate script, or network request was executed.

**The early performance screen fails; direct rejection is supported.** Affected edge complete p50 improves in **5/12**, below the predeclared eight, and small-edge complete p50 improves in **2/4**, below three. No source-contract or evidence-integrity blocker was found.

**Only three ordinary gates passed: fmt, targeted, and fixes, plus the separate work test. This candidate did not complete twelve-gate qualification.** The full-vector correctness screen passes for the saved benchmark workloads. That is sufficient evidence for rejecting this attempt under the declared early-screen workflow, not a claim that the omitted nine gates passed or that the candidate was ready for retention.

## Source and qualification boundary

Actual control is immutable `c7508eec3ac44138305797691244ae9c180d7680`, retained runtime `0e441c8`. I independently matched all 324 control-receipt source paths to its Git blobs. Candidate formal paths are identical, with exactly one changed file: graph/authority.rs. All 323 early-check source paths match the formal candidate. The extra formal path is the unchanged C++ comparator.

Independent unified-patch reconstruction matches the saved full source pair: one hunk, +13/-4. The only edit is refresh_child_placements. It borrows the immutable child roster and mutable placement field separately, instead of cloning a temporary child Vec and calling place_node for each child. Its local loop retains place_node's exact generational contains_key guard, placement value, equality check, and conditional Arc::make_mut/insert.

Missing/unplaced parents and nonsplit parents retain their original early returns. Missing or stale-generation children remain skipped. Child indices and iteration order remain unchanged; repeated children retain the same last-write behavior. Arc::make_mut still runs only for an actual placement difference, preserving the necessary first-write COW rather than moving it before the loop. The loop does not mutate graph nodes, root ownership, reference epochs, item/MRU state, or output. No other writer or validation path is changed.

This removes the explicit source-level temporary roster clone. It does not remove normative split payload ownership or necessary placement-map COW. No allocator count or exclusive explanation for the measured result is inferred.

| Artifact | SHA-256 |
|---|---|
| Base authority.rs | `89d0d337cdc2365caf62f5dd2d48d6f9132b21c2d4b882e9c24096cc6602a600` |
| Candidate authority.rs | `61245b367348b1fa80cdc1759e4e4686230f6bc83430829899571a72dec05005` |
| Exact runtime.patch | `57ec91635b6d9a146259b3f97c7808d5939420dc7d64c94b755661378161f9ee` |
| Formal source receipt | `c2ab0d375d90e79351de5f17b98f9b5d78779cfa61f3918ac4507c789c721cb3` |
| checks-fmt,targeted,fixes.json | `ae5bf3a7058917402fcd52f974658df9b7930194e0806ed5936f5d63870b5616` |
| Candidate executable | `7a459de3b9653943a2fd1693ecb1805bb481cdacd6870b67de228e20065462d0` |
| Rust control executable | `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b` |
| ImGui executable | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

The three recorded ordinary checks run serially from 12:03:21 to 12:03:50 UTC with normal counters enabled and unchanged source hashes. Formatting returns zero; targeted reports 432 passed/1644 skipped; fixes reports 4 passed/2072 skipped. I checked all saved PASS ordinals and summaries. There is no candidate receipt for production core, public integrations, workspace, Clippy, docs, doctests, dependencies, host default, or host serde. Compilation of the production test artifact and execution of the ignored timing harness do not constitute those omitted gates.

The separate work test passes one test, with 2075 skipped. Its 21×20 CSV is byte-identical to control, SHA-256 `88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4`, preserving six completed bodies and 2926 structure rows. This logical graph/payload ledger does not count the removed temporary Vec copy or allocator capacity.

The formal artifact records opt-level 3, debuginfo 0, default features, disabled debug assertions/overflow checks, and the existing timing-only counter bypass. The existing presentation_identity_frontier dead-code warning remains. No tests, references, numeric solver, benchmark body, or dependency were edited.

## Full saved-data audit

The archive contains **419 inputs plus manifest.json**, 420 unique safe regular members. Every member size/hash and original-input roundtrip matches. The other contract-review file was hashed for archive integrity without using its conclusions.

| Campaign | UTC interval | Raw rows | Complete vectors |
|---|---|---:|---:|
| Rust control pairing | 12:09:37.410740–12:09:41.456751 | 8064 | 168 |
| ImGui pairing | 12:09:41.493368–12:10:17.792090 | 8064 | 168 |
| Total | Separate complete campaigns | **16128** | **336** |

Each campaign has the original ABBA/BAAB groups, eight serial processes, 21 workloads, 48 samples, and eight configured warmups. All sixteen process receipts return zero; stderr is empty; Rust stdout records the successful ignored harness; ImGui stdout equals its CSV. Frozen executable hashes agree with every environment record and the existing files.

I independently checked each process/aggregate row, sample ordinal, structural count, alternating resize widths, phase sums, and all 168 summary rows across five phases. Each summary cell has 96 observations, using sorted indices 47/91/95 for p50/p95/max. Every absolute value, ratio, reduction, and aggregate statistic in all 84 comparison rows and measurements.json matches recomputation.

All 336 complete sample-zero vectors were parsed, including root order, DFS binary paths, bounds/four-unit child gaps, selection membership, exact node/leaf/split counts, and all 2N items exactly once. All 21 workload vectors agree byte-for-byte across products/runs; sample-zero FNV digests match their vectors. All 27 workload/width digest keys agree across all 16128 observations. Complete vector dumps exist for sample zero, not every timed sample.

The clock is unchanged: capture, checked mutation, fresh projection of all roots, and independent complete owned export. Existing full-state/rich-output and post-fixture-drop ownership assertions remain. This confirms the benchmark correctness screen; it does not turn three ordinary checks into twelve.

## Results and adverse observations

Counts below are improvements / regressions / ties in complete operation latency:

| Comparison/subset | p50 | p95 | max |
|---|---:|---:|---:|
| Rust, all 42 | 21 / 21 / 0 | 17 / 23 / 2 | 15 / 27 / 0 |
| Rust, 12 affected edge cells | 5 / 7 / 0 | 6 / 6 / 0 | 4 / 8 / 0 |
| Rust, four small-edge cells | 2 / 2 / 0 | 1 / 3 / 0 | 0 / 4 / 0 |
| Rust, 30 nonedge cells | 16 / 14 / 0 | 11 / 17 / 2 | 11 / 19 / 0 |
| ImGui, all 42 | 2 / 40 / 0 | 5 / 37 / 0 | 19 / 23 / 0 |

| All complete cells | p50 reduction range | p95 reduction range | max reduction range |
|---|---:|---:|---:|
| Rust | -8.901126% to +9.118674% | -41.205337% to +51.878434% | -574.017377% to +91.063812% |
| ImGui | -253.280000% to +12.606943% | -299.828620% to +21.703236% | -4236.450839% to +84.024913% |

All affected edge cells follow; positive reduction means improvement:

| Group | Panes/views | Control→candidate p50 ns | Reduction |
|---:|---|---:|---:|
| 1 | 16/1 | 3208→3375 | -5.205736% |
| 1 | 16/4 | 3208→3083 | +3.896509% |
| 1 | 128/1 | 14583→14292 | +1.995474% |
| 1 | 128/4 | 12208→13042 | -6.831586% |
| 1 | 1024/1 | 121750→117916 | +3.149076% |
| 1 | 1024/4 | 93417→92625 | +0.847811% |
| 2 | 16/1 | 3167→3375 | -6.567730% |
| 2 | 16/4 | 3125→2875 | +8.000000% |
| 2 | 128/1 | 14375→15000 | -4.347826% |
| 2 | 128/4 | 12167→13250 | -8.901126% |
| 2 | 1024/1 | 119375→120375 | -0.837696% |
| 2 | 1024/4 | 93042→95000 | -2.104426% |

The small one-view edge regresses in both groups while small four-view improves in both. Medium four-view edge regresses in both. There is no basis to hide the losses behind the explicit copy removal or to claim universal degradation from a mixed matrix.

Adverse tails are retained: all four small-edge maxima regress. The worst Rust p95 is group-1 128/4 center, 13042→18416 ns (-41.205337%). The worst Rust max is group-1 16/4 center, 2417→16291 ns (-574.017377%). All other p95/max negatives remain in the complete comparison table. No exclusive compiler, allocator, or scheduling cause is established.

The ImGui inventory gate passes at 30/42: small 2/14, medium 14/14, large 14/14. P50 candidate/comparator ratios range from 1.867200x–3.532800x, 1.189462x–1.653647x, and 0.873931x–1.162162x. This cannot substitute for either failed Rust edge threshold.

## Decision, restoration, and durable evidence

The predeclared rule was saved at 12:02:23.887394 UTC, before recorded checks and timing. It explicitly permits fmt+targeted+fixes+work before the performance screen, requires all twelve ordinary gates before any retention, and advances to the remaining gates and one additive confirmation only if performance is promising. It requires at least 8/12 edge and 3/4 small-edge complete-p50 improvements. No threshold has been changed or substituted after measurement.

The recorded central decision at 12:13:17.980947 UTC is `reject_restore`. Its counts and qualification limits agree with the independent audit. The failed 5/12 and 2/4 gates justify stopping here; there is no need to execute the remaining nine ordinary gates or a confirmation campaign for this rejected candidate.

Root's decision records full 324-source restoration. I independently read back authority.rs as exact control bytes, proving removal of this candidate's only changed file. At that point the current three-path WIP is in operation.rs, graph/shared_storage.rs, and graph/copy_tests.rs for a different candidate. Those files were not used as measured placement-borrow evidence or reviewed here. The retained runtime/inventory remain 0e441c8 and 30/42.

| Durable artifact | SHA-256 |
|---|---|
| Predeclared rule | `1c8dd7e4bb8832e51df400b1039d91d4b93ae95a5891c137cbde05efb034dc93` |
| evidence.tar.xz, 291416 bytes | `cb1d7b86cd8a00a0def88e6839a7ad0952f49925a3ce5d3cf1068c971d838f92` |
| Archive manifest.json | `d21380cf88e4fb2c07c2fb3ec465a235d9b55be786bfdbd9e9303c0a1ebf8161` |
| archive-check.json | `e08dddb9b88af8d3b3fd4c27dc57b4c849d7257c9d2eb34987d38b81c0194657` |
| comparisons.csv | `856049d1e31b9d075281898e82185d6f9bb2b70b99844d83ac6c59c9ced15a4f` |
| measurements.json | `58a05acc448bff10b68534d5f97b6ea78048931d358f82f5bcca4f9a3346097c` |
| Prior source design | `6bfc1d36ee73831adac25565ebf59a66865c984c7d7181d299d05c78e52136a9` |
| Other contract review, hash only | `01cb2093d1f35bf3a5a3ad81ee32e4c016b1b9500fecefc1453985a0b00b1613` |
| SHA256SUMS | `28cbbf7c4352db22fe2737d3352f5ae8001ddc548674b2dea3378e7e52bc75e2` |
| Later central decision.json | `d9d664ea914e53d8733b02511018df7ea8286f73bb74bff4ffee226424057c00` |

All six SHA256SUMS entries match. The later decision is separately hashed above. This report binds the saved evidence snapshot, not a later publication commit. The correctness screen and archive integrity pass; complete twelve-gate qualification remains unperformed, and the explicit performance gate fails.
