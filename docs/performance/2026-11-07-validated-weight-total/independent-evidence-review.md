Validated weight-total reuse — independent rejection evidence review, 2026-11-07

No evidence, source-binding or rejection-decision blocker found. Independent recomputation verifies 432 archived inputs, all twelve ordinary gates, the complete 21-by-20 work ledger, 16128 raw observations, 336 complete vectors, all 168 summary rows and all 84 comparison rows. The candidate fails the predeclared complete-median rule: 21 improvements versus the required 28. It also fails the separate selected-layout rule: only 3/6 selected layout medians improve, versus the required 4/6. The 31/42 ImGui threshold inventory satisfies a different condition and does not rescue either failure. Rejection is supported with all adverse cells retained.

The only reviewed experiment is candidate 1, `validated-weight-total`, under `B=/tmp/dockspace-imgui-twofold-20261107-ea4161e`. I did not read any subsequent candidate draft or use the live candidate2 source as evidence. Immutable control is `a46fddae12be5a81ac19144f55ab34066a745c5c`, whose audited core/configuration inputs match runtime `0e441c8` and frozen executable `3a885b...`. The source package is `B/validated-weight-total-source`; execution receipts/results are in the matching frozen/verified directories. Durable evidence is under `docs/performance/2026-11-07-validated-weight-total/`.

This review only reads immutable Git, archived source/receipts/logs/CSV and frozen executable bytes. It parses and recomputes evidence in independent inline Python without importing or executing any archived/candidate script. No Cargo, compilation, tests, formatting, timing, runner, profile, network operation or subagent was executed. Only this report was written. Source qualification and execution belong to Root; the source review alone is not called qualification or benefit.

The archive has 433 unique, safe regular members: 432 inputs and manifest.json. Every input's size and SHA-256 match the internal manifest; every input roundtrips to its declared original. Counts are 420 execution files, eight source-package files, two immutable actual-control files and two measured-candidate files. All six SHA256SUMS entries match the durable files. Archive size is exactly 360084 bytes. This is a hash-bound evidence snapshot; the article/archive were not yet committed at the review readback, so no fabricated publication commit is assigned.

| Evidence identity | SHA-256 |
| --- | --- |
| evidence.tar.xz | `78f36036714c97634af5c1846f85fc9022810b6e0c4483dc65e2ad44959cc1e0` |
| Internal 432-input manifest | `3756d309674e49c919708dfe613381b4c7a3c2c94fa5b07207613c88975625a8` |
| comparisons.csv | `22aa64b82f922439b6901f45f14f7bd5891b150a2412ce5e1a307fdd4f1091af` |
| measurements.json | `ecf3b22b1f9ced991931187e623af837f31aa8e878641b6d705c8ba514a3e34c` |
| decision.json | `2cd4622d14f59be98d44bf7777e328be5fc3b54734ddd32d028a30f8da72beab` |
| archive-check.json | `7b30c94612492029e14bb4bf7b40e619044bfeba0433a1acdc5e03ea1b65e235` |
| Independent source review, archived and durable | `cae2c5f76417e83c03e51c81fdf109aaabc7b9cacecd0fdd06402c40016a239b` |
| Article at readback | `f0def2cb9ed8106f4fee2576958b2a72425d67c416b34fcdea51126ea25bb58a` |

Both full base files equal immutable a46fdda byte-for-byte. The frozen runtime patch equals the source-package patch exactly. Independent literal hunk reconstruction produces both complete candidate files and the inverse source pair. The runtime file has nine hunks, +17/-7; the test file has one +70/-0 insertion. The unchanged remainder of the original test file is byte-identical. No dependency, lockfile, probe, reference or unrelated source is changed.

| Source | Actual-control SHA-256 | Candidate SHA-256 |
| --- | --- | --- |
| crates/dockspace/src/layout.rs | `01f21abad6c4d71856dbf17ae9eec90a97d7fbe7717bb2e6da87895f2fed5dcd` | `c6c2242176fca48d7b604c7ab5e023c57093508081d6c7c1d4535bc27ca1dc52` |
| crates/dockspace/src/layout/topology_tests.rs | `67671850120413fbaec896b7eea930b7eb52086ccf4864b8486ab21ed8ea1ad2` | `859bce05e75e7733f34db07744b5d064dbc89256b7e300350cebcc9a79be459a` |

Runtime/source patch SHA-256 is `c4ea86a96b794d8ede5445b8723932154ced2010bc91bec206a5934a011bb422`; scope is `69016fa40240c8bc1a0c1fb4d713ed1488fcab15a34aabab7ced8f389d6e9e18`; application receipt is `d75f32415841504761f63a0d0dd84f58e570f9c917d007db75c6303afee8bb91`. Source-package formatting-note hash is `15991942657a6b276906abba35cb653bde53f1aca626f741907de8acfc2d7b59`. The note identifies test-only formatting; the same final formatted test hash appears in both ordinary-check batches, the source package, formal receipt and measured snapshot. No unformatted earlier draft is used to bind execution.

The source change forwards one existing checked f64 value through the public solve_axis/private solve_axis_into callers into the sole solve_validated_axis_into executor and desired_sizes_into. validate_inputs retains exactly the original extent, length, central index, ordered individual weight checks, ordered full checked weight sum and noncentral positive-total admission. It returns that computed sum rather than unit. Both callers supply their own validator's result with the same immutable weight slice. No independent unchecked source for the scalar and no second solver are added.

On feasible noncentral desired sizing, the removed reduction is the exact same ordered `checked_sum(weights.iter().copied(), "weight")` already performed during validation. There is no callback or weight mutation between the two former reductions; sizes writes are through a distinct mutable slice. The preserved expression is still `extent * (weight / weight_total)` in child order, not a complement or reassociated expression. Minimum and maximum checks, early infeasibility results, clamping, checked assigned-size reduction, correction, exact-assigned-total reuse, tolerance and outcome calculations retain their original positions and expressions.

The central branch is independently byte-identical: the original filtered sibling sum, scale decision, sibling products, checked desired-size sum and central remainder remain. The full weight sum is still validated even if central desired sizing does not use it. It is not replaced by full_total minus central_weight. Empty and infeasible paths did not execute the removed desired-size reduction; the added scalar argument can still alter their call lowering or live-range cost. This is not a binary-only alternate evaluator, bypass of generic/nary validation, or a deletion of an allocation/output. The existing gather and stack-reserve source is unchanged outside these numeric hunks.

The frozen independent reference is unchanged, SHA-256 `4b6ff84c9bed4b65a202e1c771b5f176b161bbb27b99dba25e4875c7b08c07c6`. The new registered `binary_axis_scalar_boundaries_match_frozen_solver` is an oracle witness, not a private-implementation mirror: 11 extents × 11 weight pairs × six constraint pairs × four central choices = 2904 cases. It compares successful sizes/overflow/unallocated by bits and unsuccessful results by the existing Debug representation. Cases include signed zero, smallest subnormal/normal, finite extremes, zero/nonfinite/negative weights, aggregate overflow, invalid central index and competing errors. Debug error comparisons distinguish the relevant variants/indices/quantities but do not separately establish NaN payload-bit equality; preserving the original error-producing expressions supplies that source argument. No existing test/assertion is weakened. The new test is recorded PASS in the targeted, production and workspace logs. It is one registered test executing 2904 combinations, not 2904 independent nextest cases.

Formal candidate receipt SHA-256 is `d37b9c4d23ae8a55304a9fb2d0d93a20d1b36a96304f8a8aa14cf89d626615e7`. It records the same 324 source/configuration inputs as the frozen control receipt, differing only in the two listed source/test files. All 324 control input hashes independently match immutable a46fdda; the candidate receipt is exactly those control hashes with the two archived candidate overlays. All campaign candidate_sources equal that formal receipt. This binds the control executable to the actual retained runtime even though the frozen executable originally came from 0e441c8.

| Executable | Size, bytes | Independently verified SHA-256 |
| --- | ---: | --- |
| Candidate | 18750960 | `2eb0e3825e795a47c5d5e4bb59f699388c2d3fcc52927d921f67cb9099bad3dc` |
| Actual retained control | 18750720 | `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b` |
| Pinned ImGui | 503280 | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

These files were hashed, not executed. Candidate build JSON identifies its lib-test artifact with optimization 3, no debug information, debug assertions false and overflow checks false. The formal freeze uses production profile, one build job and `DOCKSPACE_BENCH_NO_WORK_COUNTERS=1`. Ordinary validation and the work probe explicitly remove this bypass. The existing presentation_identity_frontier dead-code warning remains; no warning-free claim is made. The 240-byte executable-size difference is an identity detail, not evidence of inlining, register pressure, instruction count or an exclusive compiler cause.

All twelve ordinary gates are supported by their original two invocations on precisely the same 323 source/configuration hashes. `checks-targeted.json` records the first invocation and its 433 passing tests; the second receipt records the other eleven gates. Their complete source maps equal each other and are a subset of the candidate formal source map. Every original entry matches its checks-all.json entry exactly, with no duplicate or missing gate. Original receipts, logs and the synthesized aggregate remain archived.

The aggregate receipt's started_utc is inherited from the second invocation at `07:24:45.199340`; the targeted entry actually starts at `07:23:06.780252` (first receipt starts 07:23:06.780093). This is a documented merge, not one twelve-gate process whose listed start predates every entry. The original per-invocation times are available and consistent. This metadata limitation does not change source identity or PASS evidence. The aggregate/source identity is SHA-256 `fb61f7396cd927883a9fb54109bdbf763e846593fd4ca90fd388a3f9a4173344`; targeted receipt is `147fbee561042bf9f13b80c27bc2987c49450ff5bcbc109523ac0c0f0880cc4a`; remaining-eleven receipt is `bd2e30dbc8dee9b071f941064d4bb547d8d20864546eff0a19ad3db8f5a6055b`.

Independent numbered-PASS parsing agrees with every nextest summary:

| Gate | Passed | Skipped |
| --- | ---: | ---: |
| Targeted | 433 | 1644 |
| Four existing fixes | 4 | 2073 |
| Production core | 2074 | 3 |
| Public integrations | 4 | 0 |
| Workspace | 2250 | 3 |
| Host default | 292 | 0 |
| Host serde | 335 | 0 |

The remaining five gates are formatting, correctness/suspicious clippy, strict docs, workspace doctests and core dependency checking, each with return code zero. Doctest summaries are 9, 0 and 1 passed, with zero failures. No anchored FAIL/TIMEOUT/ABORT/CANCELLED result is present. Clippy intentionally allows other warnings; docs uses `RUSTDOCFLAGS=-D warnings`. Work is separate: one PASS, 2076 skipped, return code zero. The 21×20 CSV is byte-identical to the actual-control work ledger, SHA-256 `88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4`. Every counter was independently checked: resize zero work; moves one candidate/fork, `2N-V` cloned nodes/handles, `2N` cloned tab items and two payloads; item/MRU copies four for center/cross or two for edge. Edge retains two child/weight copies, one completed root with `2*(N/V)+1` rows and `N/V` handles. Total six completed roots/2926 rows remains unchanged. These logical-work counters do not measure f64 reductions, allocator traffic or total CPU cost.

Scope is recorded at `2026-11-07T07:23:04.941111+00:00`, before either ordinary-check batch, formal build at 07:41:59.996868, actual-control campaign at 07:45:19.896802 or ImGui campaign at 07:45:24.156427. The decision is recorded after both campaigns at 07:46:47.599193. The archived rule requires at least 28/42 complete p50 improvements; at least 30/42 ImGui cells at most 2x including every medium/large cell; at least 4/6 selected one-view small resize/center/edge layout p50 improvements; and publication of negative/tail results. This is a recorded pre-measurement rule, not an externally timestamped commitment.

Both measured campaigns include all eight serial ABBA/BAAB processes, 21 workloads/process, 48 numbered samples/workload and eight configured warmups. Counts are 1008 rows/process, 8064/reference campaign and 16128 total; each condition/group/workload has 96 observations. Every process CSV matches its exact ordered aggregate slice. All process returns are zero; stderr is empty; Rust stdout records its one harness test PASS; ImGui baseline stdout equals its CSV. Recorded process intervals do not overlap. All cardinalities, products, sample ordinals and alternating resize widths validate. Every complete duration equals request + mutation + layout + export.

The formal probe source is unchanged (`docking_cost.rs` SHA-256 `01d8438fc79095aacae935170aff3ed7eb8b5e497aa368c9966889b789adf2d3`). Its existing interval includes request capture, checked mutation, fresh all-root projection and owned complete export. No capture, mutation, output or validation contract is removed to produce these observations. Benchmark state checking remains outside the interval. Source hashes also preserve the C++ comparator and dependency configuration byte-for-byte.

All 336 complete vectors were parsed, not merely counted: complete ordered DFS forest, root and binary-child paths, parent/child bounds and four-unit gaps, valid tab selections, exactly-once item inventory, and structural totals are valid. All eight processes in each campaign and both products have identical bytes for each of the 21 workload vectors. Independently recomputed little-endian-u64 FNV-1a matches sample-zero raw digests. All 27 workload/width digest classes agree across all 16128 rows. Sample-zero vector files are not misrepresented as full dumps for every alternating width/sample.

All 168 summary rows for all five phases were independently rebuilt from raw data: nearest-rank p50 uses index 47, p95 index 91, max index 95 of 96 values. All 84 complete-operation comparison rows, including absolute times/ratios/signed reductions, match independent recomputation. The decision and measurements reproduce the same statistics:

| Reference | Metric | Improve | Regress | Tie | Signed reduction range, percent |
| --- | --- | ---: | ---: | ---: | ---: |
| Actual a46 control | p50 | 21 | 20 | 1 | -4.278104 to +7.229612 |
| Actual a46 control | p95 | 15 | 25 | 2 | -17.580279 to +30.087805 |
| Actual a46 control | max | 21 | 21 | 0 | -522.122225 to +79.375939 |
| ImGui | p50 | 2 | 40 | 0 | -225.860271 to +13.016276 |
| ImGui | p95 | 5 | 37 | 0 | -371.880998 to +13.737263 |
| ImGui | max | 10 | 32 | 0 | -655.600000 to +92.410714 |

All cell results are retained below, including favorable and adverse tails. Signed reduction is `100*(1-candidate/reference)`; negative means slower. The actual-control and ImGui columns come from separate campaigns and do not share absolute timing observations.

| Group | Panes | Views | Workload | Control p50 % | Control p95 % | Control max % | ImGui p50 % | ImGui p95 % | ImGui max % |
| --- | ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 16 | 1 | resize | -3.4768 | -9.3773 | -33.3091 | -86.7200 | -143.6282 | -135.1199 |
| 1 | 16 | 1 | center_move | -1.9391 | -17.5803 | -265.8180 | -188.9333 | -219.9616 | -5.5556 |
| 1 | 16 | 1 | edge_split | +1.2291 | +10.9117 | +79.3759 | -203.9705 | -207.1123 | -144.8580 |
| 1 | 16 | 4 | resize | +0.0750 | +5.2432 | +55.9226 | -121.2329 | -112.4438 | -95.3143 |
| 1 | 16 | 4 | center_move | -2.0078 | -5.6134 | +1.2597 | -225.3754 | -234.9788 | -655.6000 |
| 1 | 16 | 4 | edge_split | -1.3623 | -2.5213 | +6.7159 | -225.8603 | -371.8810 | -588.6427 |
| 1 | 16 | 4 | cross_view_move | -1.9765 | -1.8325 | -6.1277 | -218.5907 | -234.9788 | -261.9429 |
| 1 | 128 | 1 | resize | +0.9055 | +0.4402 | -27.1230 | -26.9053 | -1.7202 | -341.0667 |
| 1 | 128 | 1 | center_move | +2.7269 | -7.8950 | -32.6204 | -38.9831 | -44.0708 | +71.5449 |
| 1 | 128 | 1 | edge_split | +2.0867 | -3.5893 | -123.6243 | -46.8582 | -20.2922 | -33.5530 |
| 1 | 128 | 4 | resize | -0.8932 | -0.4289 | +1.4314 | -34.1578 | -10.6839 | +85.7066 |
| 1 | 128 | 4 | center_move | +1.9593 | +5.1902 | +14.3614 | -50.8837 | -56.9080 | -603.1147 |
| 1 | 128 | 4 | edge_split | +2.4744 | -4.8966 | -137.7646 | -27.5753 | -7.1626 | +76.4844 |
| 1 | 128 | 4 | cross_view_move | +3.7678 | -0.3104 | +53.9798 | -47.6634 | -46.2323 | -36.6753 |
| 1 | 1024 | 1 | resize | -0.0014 | +1.6389 | -18.3218 | -2.4779 | +2.2377 | -95.2141 |
| 1 | 1024 | 1 | center_move | -0.5779 | +4.2181 | -7.0292 | -6.8430 | -7.4444 | -38.8021 |
| 1 | 1024 | 1 | edge_split | +0.6141 | -2.6331 | -10.5046 | -9.2964 | -16.0343 | -321.5919 |
| 1 | 1024 | 4 | resize | +2.0067 | +12.9608 | +55.4916 | -3.8253 | -3.6765 | +24.2231 |
| 1 | 1024 | 4 | center_move | -0.5256 | +0.0000 | -101.5921 | -13.5996 | -8.0353 | -43.8049 |
| 1 | 1024 | 4 | edge_split | -0.6199 | -2.6894 | -22.3389 | +12.8853 | +13.7373 | +53.0257 |
| 1 | 1024 | 4 | cross_view_move | +0.6266 | +21.4105 | +2.8760 | -12.6862 | -8.0476 | -12.4250 |
| 2 | 16 | 1 | resize | +0.0827 | -7.8283 | +19.2432 | -81.1094 | -95.2000 | +92.4107 |
| 2 | 16 | 1 | center_move | +5.5556 | +6.5467 | +1.4128 | -172.2667 | -336.7424 | -471.8810 |
| 2 | 16 | 1 | edge_split | +7.2296 | +30.0878 | +77.2519 | -170.4000 | -248.2972 | -453.8911 |
| 2 | 16 | 4 | resize | -3.3600 | +2.9640 | -2.5846 | -100.0000 | -94.2090 | -100.1264 |
| 2 | 16 | 4 | center_move | -2.0078 | +1.8667 | +4.6160 | -206.1469 | -130.5846 | +85.5412 |
| 2 | 16 | 4 | edge_split | +1.3299 | +3.7037 | -3.0615 | -200.0000 | -185.2444 | -185.4520 |
| 2 | 16 | 4 | cross_view_move | -3.9347 | -1.8325 | +7.1649 | -212.4438 | -200.0000 | +50.7521 |
| 2 | 128 | 1 | resize | +0.0000 | -5.2860 | -37.1467 | -25.5931 | -17.8856 | -37.1241 |
| 2 | 128 | 1 | center_move | +1.1905 | -12.6802 | -4.2789 | -45.6513 | -61.0459 | -192.3160 |
| 2 | 128 | 1 | edge_split | -1.8574 | -10.2220 | +1.2121 | -46.6635 | -68.6577 | -67.1291 |
| 2 | 128 | 4 | resize | +1.7761 | -1.2707 | -522.1222 | -41.1148 | -17.3221 | -171.1613 |
| 2 | 128 | 4 | center_move | +1.9183 | +1.1030 | -3.2909 | -51.4873 | -35.4125 | -12.2430 |
| 2 | 128 | 4 | edge_split | +3.3926 | -3.2046 | +31.2139 | -28.9646 | -30.3000 | -300.3225 |
| 2 | 128 | 4 | cross_view_move | +1.1812 | -7.7778 | +16.7857 | -45.0386 | -36.7024 | -29.3895 |
| 2 | 1024 | 1 | resize | -0.4084 | +0.0000 | +3.8278 | -2.5299 | +6.1828 | -108.7606 |
| 2 | 1024 | 1 | center_move | -3.1209 | -12.6922 | -4.6223 | -8.5739 | -3.1992 | -9.7316 |
| 2 | 1024 | 1 | edge_split | -2.3793 | -11.7806 | -15.0689 | -9.2955 | -11.6206 | -37.4743 |
| 2 | 1024 | 4 | resize | -0.8489 | -1.5568 | +24.7084 | -9.3425 | +0.4055 | +51.4662 |
| 2 | 1024 | 4 | center_move | +1.3905 | -3.6197 | +47.7703 | -14.7526 | -13.3129 | -0.4018 |
| 2 | 1024 | 4 | edge_split | -4.2781 | -1.5733 | -9.9041 | +13.0163 | +8.6284 | -60.1437 |
| 2 | 1024 | 4 | cross_view_move | -1.9767 | +6.0519 | +27.9148 | -12.0490 | -7.0758 | +12.5398 |

The worst actual-control median is group 2, 1024/four-view edge: 92541 to 96500 ns (-4.278104%). The worst p95 is group 1, 16/one-view center: 3083 to 3625 ns (-17.580279%). The worst maximum is group 2, 128/four-view resize: 11667 to 72583 ns (-522.122225%). None is removed as an outlier. The aggregate result also conceals a size difference worth preserving: small medians are 6 improvements/8 regressions, medium 11/2/1, large 4/10. The source deletion therefore does not establish a broad benefit, even with favorable medium medians or a higher threshold inventory.

The independently reconstructed six preselected layout medians are:

| Group | Panes/views/workload | Control layout p50 ns | Candidate layout p50 ns | Result |
| --- | --- | ---: | ---: | --- |
| 1 | 16/1 resize | 959 | 1000 | Regress |
| 1 | 16/1 center | 1000 | 1042 | Regress |
| 1 | 16/1 edge | 1167 | 1166 | Improve |
| 2 | 16/1 resize | 958 | 958 | Tie |
| 2 | 16/1 center | 1041 | 1000 | Improve |
| 2 | 16/1 edge | 1208 | 1083 | Improve |

Only three improve; a tie does not count as the fourth required improvement. The article and decision already have a sufficient rejection reason in the 21-versus-28 complete-median failure. This additional failed layout clause strengthens that decision and is explicitly retained by this audit.

| Panes | Complete p50 candidate / ImGui range | At most 2x |
| --- | ---: | ---: |
| 16 | 1.811094–3.258603x | 3/14 |
| 128 | 1.255931–1.514873x | 14/14 |
| 1024 | 0.869837–1.147526x | 14/14 |

The target count is exactly 31/42, including all medium/large conditions. Passing small cells are one-view resize in both groups and four-view resize in group 2, the latter exactly 2.0x; eleven small cells still miss. This observed threshold improvement is from a separate ImGui campaign and is not 31 control-relative speedups, a reason to lower the predeclared 28-improvement gate, or an improvement attributed uniquely to scalar reuse.

The decision rejects this standalone treatment and records two restored source hashes: layout.rs `01f21abad6c4d71856dbf17ae9eec90a97d7fbe7717bb2e6da87895f2fed5dcd` and topology_tests.rs `67671850120413fbaec896b7eea930b7eb52086ccf4864b8486ab21ed8ea1ad2`. Both independently equal immutable a46fdda and the archived actual-control/base pair. The exact archived patch reconstructs that pair from the candidate, so the restoration boundary removes both the scalar plumbing and its new witness while preserving all pre-existing control tests, exact assigned-total reuse, binary gather and stack reserve. The decision's `restored_exact_control` is the historical restoration record; the archive does not contain a separate shell-transcript attestation of the inverse apply. This review verifies the recorded hashes and source-pair relation and deliberately does not inspect or overwrite later live work to re-observe a past state.

No dependency/configuration/native source is part of this rejected two-path patch. This is not a rollback of retained native or other runtime fixes. The review expresses no view about any subsequent candidate and does not carry this candidate's source qualification or performance result forward to it.

The source remains a valid narrow semantic experiment, and its measured rejection is correctly preserved. A repeated reduction was removed only on feasible noncentral desired sizing; an extra f64 argument and longer live value may also alter code generation, while central/empty/infeasible paths do not execute the deleted work. Neither source inspection nor this matrix proves inlining occurred, an exclusive ABI/register/allocator cause, or that checked total reuse is intrinsically slow. The evidence supports rejecting this concrete binary/source/protocol pair under its original rule.
