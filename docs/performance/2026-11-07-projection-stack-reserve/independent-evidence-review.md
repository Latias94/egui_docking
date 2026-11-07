Projection stack reserve — independent evidence review, 2026-11-07

No evidence or source-binding blocker found. Independent recomputation agrees with the complete archive, measurements, decision and published negative results. The recorded numerical retention conditions are satisfied: 28/42 complete p50 improvements, all medium/large ImGui conditions still at most 2x, inventory 30/42, and layout p50 improvements in both groups of both preselected diagnostic workloads. This remains a bounded result with 12 median regressions, 18 p95 regressions and 17 maximum regressions against the actual control. The qualitative tail rule has no numerical cutoff; satisfying the numerical conditions does not establish uniform or repeatable tail improvement.

This review reads immutable Git objects, the frozen executables as bytes, and archived text/CSV/JSON. It independently reconstructs statistics and validates complete vectors without executing or importing any archived or candidate script. No Cargo, build, test, timing, sampler, disassembler, workload, runner, network operation or subagent was used. Only this report was written. Final stop opinions were not read. Existing source review is a separate source-only review; its presence is not used as an execution receipt.

The retained source and durable evidence are now byte-bound to commit `0e441c8ddeb6f85b9b0e8237303b5def32e178ae`. The actual comparison control is immutable `49baba1b2b7ad8e152e8b4da180a2e130d5fce9f`. This is the reserve experiment, not the older gather binary's original-baseline confirmation, a diagnostic build, or a later live workspace.

The complete source delta is one hunk in `crates/dockspace/src/layout.rs`: replace the initial one-element `vec!` with `Vec::with_capacity(topology.nodes.len().min(8))`, then push the identical `ProjectionFrame::Enter { row: 0, bounds }`. A whole-file reconstruction matches the archived measured candidate and commit 0e441c8; every other byte of that file equals the actual 49b control. Thus binary measurement gathering, exact assigned-total reuse, full validation, arithmetic, frame processing order, child/bounds and gap order, raw-DAG traversal and fresh owned output all remain present. No dependency, public interface, custom container or unsafe code is added. The `.patch` in the source package uses temporary absolute file names; the frozen repo-path runtime patch has the same single +2/-1 hunk, and both reconstruct the exact base/tree pair.

| Identity | SHA-256 |
| --- | --- |
| Actual-control layout.rs | `ed2d1aaa966e89f94ebcc5d78f5deb4ad6e3b27a5d4d52a16d89af6da419dbf8` |
| Proposed / measured / retained layout.rs | `01f21abad6c4d71856dbf17ae9eec90a97d7fbe7717bb2e6da87895f2fed5dcd` |
| Source-package source.patch | `a02ad681a08ab7c262c9e233319d0edb2cef6c9a0643d67b55a83cd1143036ec` |
| Frozen runtime.patch | `c7c97ca62a98971d86b0365d8e76eff93bf99385e3f556bddec0f0f44e67e2ac` |
| Scope and predeclared rule | `6a0c3badd138c99148485f5e51b69c33a358e15842736e4880b693141278f876` |
| Source review, archived and durable copy | `0eb03a713785232b073e447e94130c860327f6fc3a395026b6cedf8026817b15` |
| Actual application receipt | `11a44f19ee4b157dd38954b2870dfb307fb31285a06fa6a8decd99f28f1f67e9` |
| Formal candidate source receipt | `03a4404a1fa9ed1f38a8aad04e3cef4a64e4f984b0d9ff7bb55d758f6ea262f6` |
| Normal checks receipt | `a450a31b189632d86a9e93176f426eac630b5af0a2e5b7db65709d4e349be8f5` |
| Candidate executable | `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b` |
| Actual-control gather executable | `6d255458eac25cd9dcf629b88cfca6959c78db7ed2bb7ccf08fe2cba6332dcfd` |
| Pinned ImGui executable | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

All three executable identities were independently hashed without execution. Candidate and actual-control executables are each 18,750,720 bytes; ImGui is 503,280 bytes. All 324 candidate source/configuration inputs match immutable 0e441c8. The candidate and actual-control formal receipts differ only in layout.rs. The normal validation receipt has the same 323 Rust/configuration inputs; the formal set additionally includes the unchanged C++ comparator. Compiler JSON records the candidate's lib-test artifact, optimization level 3, no debug information, debug assertions false and overflow checks false. Freeze records production profile and `DOCKSPACE_BENCH_NO_WORK_COUNTERS=1`; ordinary validation/work explicitly remove that environment variable. This distinction is maintained throughout. The existing `presentation_identity_frontier` dead-code warning is present; this review does not call the build warning-free.

The archive at `docs/performance/2026-11-07-projection-stack-reserve/evidence.tar.xz` contains 426 unique, safe regular members: 425 inputs and manifest.json. All member sizes and SHA-256 values independently match the manifest. Every input also roundtrips to its declared original: 418 execution files, five source-package files, one immutable actual-control file and one proposed measured-candidate file. The six SHA256SUMS entries match. The tar, published measurements/comparisons/decision/check metadata/source review, SHA256SUMS and article all match commit 0e441c8.

| Durable identity | SHA-256 |
| --- | --- |
| Evidence tar, 357,528 bytes | `8d73d37dd288875b3ce2b3ac55b5c9fde3919ae9e7290938507573e0701be814` |
| Internal manifest | `984f8919c9008253bc3720f359ebd5a79eae4dd4e25ce44876b8910a851914a4` |
| comparisons.csv | `302149ebd41902184578b737270bcd3d5b5c1746285e0986a62b45fa0250d124` |
| measurements.json | `aece245ee746e2f5c22c991a989aaa37fca56ca15aa0c942f95e519b4edfcc88` |
| decision.json | `0e73e40cc01fe2adb4e74daf50fe107302fa7759d2b05f72aa757e6be2dc8dc3` |
| archive-check.json | `81f8be30167a30ca6129467897d09265557cac900f987260027ee831677e0bba` |
| Article | `a0944b56b6826abfa44af4083e05d081f578d8165bf10165a4f087cec47e7195` |

The retention rule is recorded at `2026-11-07T05:30:16.177852+00:00`, before formal build start `05:40:28.395033`, the actual-control campaign start `05:43:51.500990`, and the ImGui campaign start `05:43:55.829909`. All 16 measurement process starts follow that record. Scope's rule equals decision.json's rule exactly, including the two workload selections, publication of negative tails, and one treatment with no cap search. Ordinary qualification had already started at 05:23:49; the supported claim is pre-measurement declaration, not declaration before all validation or an externally sealed timestamp. The record describes one cap treatment; the audit does not infer otherwise unrecorded experiments.

The twelve gates all record return code zero. Independent log parsing checks all numbered PASS records against their nextest summaries, rather than trusting the aggregate receipt alone:

| Gate | Passed | Skipped |
| --- | ---: | ---: |
| Targeted | 432 | 1644 |
| Four existing fixes | 4 | 2072 |
| Production core | 2073 | 3 |
| Public integrations, no default features | 4 | 0 |
| Workspace, all features/all targets | 2249 | 3 |
| Host default | 292 | 0 |
| Host serde | 335 | 0 |

The remaining five gates are formatting, clippy correctness/suspicious, strict docs, workspace doctests and dependency boundary checking. Doctest summaries are 9, 0 and 1 passed, with zero failures. No anchored FAIL/TIMEOUT/ABORT/CANCELLED result was found. Docs records `RUSTDOCFLAGS=-D warnings`; clippy deliberately allows other warnings while denying correctness/suspicious lints. The work probe separately records one passing test and 2075 skipped. Its 21-row, 20-column CSV is byte-identical to the actual control, SHA-256 `88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4`. All cells were independently checked: resize counters are zero; commands keep one candidate/fork, `2N-V` cloned nodes/handles, `2N` cloned tab items, two cloned payloads, four item/MRU copies for center/cross and two for edge. Edge alone has two child/weight copies and one completed root with `2*(N/V)+1` rows and `N/V` split handles. The totals remain six bodies and 2926 rows. These are logical-work counters, not allocator/capacity-byte measurements.

Both reference campaigns contain eight serial ABBA/BAAB processes, each with 21 workloads, 48 numbered samples and eight configured warmups: 1008 observations/process, 8064/campaign, 16128 in total. Each process CSV independently matches its slice of aggregate raw.csv, all process return codes are zero, stderr is empty, Rust stdout reports its one harness test passing, and ImGui baseline stdout equals its CSV. Recorded intervals are serial. Neither campaign is truncated or selected to only favorable cells.

For every raw row, the four phase times exactly sum to `operation_owned_ns`; workload/cardinality fields, alternating resize widths, sample ordinals and product identity also validate. The unchanged timed Rust function covers capture_request, checked apply_request, fresh project_all_roots and independently owned export_state through the exported timestamp. Fixture creation and subsequent state assertions remain outside that interval. The control and treatment measure the same complete operation; isolated layout medians are used only for the preselected location checks.

All 336 complete vector files were parsed independently, not merely hashed. They encode the expected complete DFS forest, root order and binary child paths, valid 4-unit gaps/bounds, tab selections, every item 1 through 2N exactly once, and the correct node/leaf/split/root totals. The vectors agree byte-for-byte across both products, conditions, groups and runs for all 21 workloads. Recomputed little-endian-u64 FNV-1a matches each process's sample-zero digest. All 27 workload/width digest classes agree across all 16128 observations. This does not imply vector files were emitted for every resize width; complete files are sample-zero exports, with all rows additionally checked by their digest and structural metadata.

All 168 summary rows were reconstructed from raw data for all five phases using nearest-rank p50 index 47, p95 index 91 and maximum index 95 of the 96 observations/condition/group/workload. All 84 published comparison rows, including absolute values, ratios and signed reductions for all three complete metrics, match independently recomputed values. Measurements and decision reproduce the same full statistics:

| Reference | Metric | Improve | Regress | Tie | Reduction range, percent |
| --- | --- | ---: | ---: | ---: | ---: |
| Actual 49b control | p50 | 28 | 12 | 2 | -3.934741 to +9.079005 |
| Actual 49b control | p95 | 23 | 18 | 1 | -79.095730 to +61.153315 |
| Actual 49b control | max | 25 | 17 | 0 | -444.030913 to +86.363388 |
| ImGui | p50 | 4 | 38 | 0 | -212.443778 to +17.083787 |
| ImGui | p95 | 7 | 35 | 0 | -352.398990 to +18.478052 |
| ImGui | max | 15 | 27 | 0 | -2109.595960 to +79.982754 |

All cells are retained below. Signed reduction is `100*(1-candidate/reference)`; negative means slower. The two sets of columns come from separate campaigns and must not be combined into absolute cross-campaign timings.

| Group | Panes | Views | Workload | Control p50 % | Control p95 % | Control max % | ImGui p50 % | ImGui p95 % | ImGui max % |
| --- | ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 16 | 1 | resize | -3.3600 | +0.0000 | +26.4040 | -81.5315 | -93.9351 | -105.1621 |
| 1 | 16 | 1 | center_move | +1.9022 | -7.2296 | -360.7896 | -205.6417 | -352.3990 | -1333.7852 |
| 1 | 16 | 1 | edge_split | +0.0000 | -19.0887 | -217.1715 | -203.6444 | -239.1602 | -223.7524 |
| 1 | 16 | 4 | resize | +8.5734 | -5.3826 | +2.2901 | -113.2800 | -143.6282 | -2109.5960 |
| 1 | 16 | 4 | center_move | +9.0790 | +61.1533 | +86.3634 | -193.7031 | -205.6417 | -199.8737 |
| 1 | 16 | 4 | edge_split | +6.3487 | +13.4092 | -9.0988 | -208.3000 | -223.8964 | -454.8387 |
| 1 | 16 | 4 | cross_view_move | -3.9347 | +5.0448 | +9.9910 | -212.4438 | -229.0550 | -186.5721 |
| 1 | 128 | 1 | resize | +0.4603 | -3.4749 | -2.8542 | -22.8470 | -6.4186 | +78.3811 |
| 1 | 128 | 1 | center_move | -2.4096 | +5.3489 | +13.2831 | -37.5812 | -12.5078 | +33.3333 |
| 1 | 128 | 1 | edge_split | -1.0652 | +27.0270 | +58.1722 | -46.9307 | -64.6147 | -361.1091 |
| 1 | 128 | 4 | resize | +4.6578 | -14.2244 | -28.4667 | -33.1298 | -8.2192 | +49.1453 |
| 1 | 128 | 4 | center_move | +3.7765 | +11.6249 | +82.2808 | -48.8210 | -50.5473 | -631.1631 |
| 1 | 128 | 4 | edge_split | +2.1832 | -2.9522 | -313.1859 | -29.3205 | -20.8668 | +19.0054 |
| 1 | 128 | 4 | cross_view_move | +3.3836 | -12.4170 | -24.0514 | -47.9978 | -44.0985 | -79.0742 |
| 1 | 1024 | 1 | resize | +0.2843 | +1.0447 | +53.5288 | +0.4032 | +0.7310 | -279.0403 |
| 1 | 1024 | 1 | center_move | -0.3613 | -0.9398 | +24.4720 | -5.6656 | -3.7560 | -65.3766 |
| 1 | 1024 | 1 | edge_split | +0.7130 | +29.4371 | -24.3298 | -4.1735 | -10.1454 | +25.8142 |
| 1 | 1024 | 4 | resize | -0.7203 | -3.7821 | -71.1342 | -3.3431 | -0.4511 | +6.1602 |
| 1 | 1024 | 4 | center_move | +0.1040 | -6.9145 | -95.9629 | -14.4517 | -17.3299 | -41.2835 |
| 1 | 1024 | 4 | edge_split | -1.1041 | -8.3677 | +19.4446 | +16.3100 | +18.4781 | +42.0848 |
| 1 | 1024 | 4 | cross_view_move | -0.4658 | -2.3252 | +45.8109 | -8.3609 | -2.6512 | +8.6955 |
| 2 | 16 | 1 | resize | +3.3940 | +3.1508 | +46.5232 | -81.1094 | -52.4571 | -754.1985 |
| 2 | 16 | 1 | center_move | +1.9382 | +6.3203 | +14.5320 | -177.7333 | -162.5000 | -163.2800 |
| 2 | 16 | 1 | edge_split | +1.2758 | -79.0957 | -444.0309 | -182.2470 | -129.7859 | -95.9693 |
| 2 | 16 | 4 | resize | +8.7571 | +8.3333 | +20.9263 | -113.2800 | -112.4438 | -86.2595 |
| 2 | 16 | 4 | center_move | +3.8763 | +1.8325 | +8.3333 | -206.1469 | -217.3484 | -179.9424 |
| 2 | 16 | 4 | edge_split | +5.1746 | +56.5343 | +73.0173 | -208.3000 | -182.3328 | -254.7214 |
| 2 | 16 | 4 | cross_view_move | +7.2457 | +4.6160 | -26.6657 | -212.4438 | -165.1861 | +43.1529 |
| 2 | 128 | 1 | resize | +0.0000 | +2.9949 | +79.5528 | -23.1271 | -15.1125 | +79.9828 |
| 2 | 128 | 1 | center_move | -0.8033 | -0.3799 | -11.3447 | -37.9269 | -31.5531 | +29.6395 |
| 2 | 128 | 1 | edge_split | +0.2631 | +3.2146 | +36.8784 | -45.3080 | -55.1020 | -573.2171 |
| 2 | 128 | 4 | resize | +2.1333 | +3.9153 | +82.8683 | -31.3939 | -31.7460 | -73.9726 |
| 2 | 128 | 4 | center_move | +0.7724 | +15.7208 | +57.1220 | -46.8368 | -29.6016 | -12.9200 |
| 2 | 128 | 4 | edge_split | -0.3140 | -2.6875 | +32.5933 | -25.5020 | -17.0213 | -42.7273 |
| 2 | 128 | 4 | cross_view_move | +0.3847 | +4.2702 | -20.0593 | -49.6879 | -33.1879 | -331.3485 |
| 2 | 1024 | 1 | resize | +1.1334 | +2.2412 | +15.7644 | +1.9859 | +7.6996 | +50.3302 |
| 2 | 1024 | 1 | center_move | -0.2111 | -5.7952 | -78.9110 | -3.5202 | -22.5799 | -103.0597 |
| 2 | 1024 | 1 | edge_split | -0.3069 | -7.2735 | +13.0440 | -1.5935 | -4.5208 | +28.4592 |
| 2 | 1024 | 4 | resize | +1.4743 | +2.2468 | +27.0504 | -0.8591 | +3.7424 | -19.9952 |
| 2 | 1024 | 4 | center_move | +1.1833 | +8.4260 | -38.5509 | -8.1405 | +2.5019 | +1.6541 |
| 2 | 1024 | 4 | edge_split | +1.0435 | -6.2830 | -111.5693 | +17.0838 | +10.5450 | -20.0190 |
| 2 | 1024 | 4 | cross_view_move | +2.5662 | +1.9157 | +28.3033 | -7.3853 | +4.2451 | +21.1916 |

The worst control median regression is group 1, 16 panes/four views/cross-view move: 2084 to 2166 ns (-3.9347%). The worst control p95 and maximum are group 2, 16 panes/one view/edge: 3583 to 6417 ns (-79.0957%) and 4917 to 26750 ns (-444.0309%). Another small center maximum rises 4458 to 20542 ns. These large relative tail penalties remain visible despite the favorable selected layout medians. No outlier was removed, winsorized or reclassified, and capacity is not assigned an exclusive allocator or compiler cause.

The preselected location checks independently reconstruct as follows:

| Group | Panes/views/workload | Layout p50, control → candidate ns | Complete p50, control → candidate ns | Complete reduction |
| --- | --- | ---: | ---: | ---: |
| 1 | 16/4 resize | 1167 → 1042 | 1458 → 1333 | +8.573388% |
| 2 | 16/4 resize | 1166 → 1041 | 1416 → 1292 | +8.757062% |
| 1 | 16/1 edge | 1250 → 1167 | 3417 → 3417 | 0% |
| 2 | 16/1 edge | 1208 → 1166 | 3292 → 3250 | +1.275820% |

Thus both selected layout checks improve in both groups, while the edge complete operation is only tied/modestly improved. This cannot be restated as a general complete-operation reduction matching the layout reduction.

| Panes | Complete p50 candidate / ImGui range | At most 2x |
| --- | ---: | ---: |
| 16 | 1.811094–3.124438x | 2/14 |
| 128 | 1.228470–1.496879x | 14/14 |
| 1024 | 0.829162–1.144517x | 14/14 |

The inventory is exactly 30/42. The only two passing small conditions are single-view resize, one per group; twelve small conditions remain above 2x. All medium/large conditions remain within target. This threshold is candidate versus ImGui, not a count of improvements against 49b. Neither 30/42 nor the selected cases erase the other negative cells.

Capacity cost remains explicit. Standard Vec still allocates per root, initializes the same first frame and grows normally when needed. The bounded node-count hint is not a traversal bound, depth validation, retained cache or output reuse. At the observed 48-byte frame layout, a seven-node root requests seven slots (336 bytes), compared with the former eventual four-slot capacity (192 bytes): 144 additional temporary requested bytes for the active root. A balanced 31-node single root requests eight, matching its former eventual capacity and avoiding intervening growth. Deep/skewed/nary/raw inputs retain ordinary growth and can request more later. Allocation-class rounding, locality and generated code can cost more; the source change does not establish fewer allocations or lower memory/latency for every root shape. Diagnostic stack observations locate the hypothesis and provide no time share, numerical lower bound or exclusive causal attribution for the measured medians/tails.

The retained source boundary is exactly this one initialization delta on top of the gather and exact-total control. The archived gates and work counts support compatibility, full vectors support the measured workload equivalence, and the full matrix supports the stated bounded retention decision. No claim is made that every shape improves, that all tails improve, that the 42-cell goal is complete, or that source review alone constitutes qualification.
