Final retained runtime versus the original baseline — independent confirmation evidence review, 2026-11-07

No evidence-binding, count, state-equivalence or arithmetic blocker found. The actual final reserve executable is compared with the exact original frozen executable. Independent recomputation confirms all 8064 observations, 168 complete state vectors, 84 five-phase summary rows and 42 complete-operation comparison rows. P50 and p95 improve in all 42 cells. Maximum improves in 41 cells and regresses in one: group 2, 16 panes/four views/edge, 29041 to 39042 ns, a signed reduction of -34.4375193692%. This adverse maximum is part of the final result and must remain public.

The reviewed campaign is `/tmp/dockspace-imgui-twofold-20261107-ea4161e/final-retained-confirmation/against-original`, with its adjacent `original-comparisons.csv`, `original-confirmation.json`, copied formal receipt and evidence archive. This replaces the earlier 49b runtime's original-baseline confirmation for statements about the final retained runtime. The older confirmation remains historical; its all-maximum improvement result does not describe this campaign. No final stop opinions were read.

The final source is immutable `0e441c8ddeb6f85b9b0e8237303b5def32e178ae`, including the bounded projection-stack reserve on top of runtime 49baba1. The original source is immutable `ea4161e221ff8687f344b98f4223dec1efa38873`. Frozen executable bytes were independently hashed without execution:

| Identity | Size, bytes | SHA-256 |
| --- | ---: | --- |
| Final retained executable | 18,750,720 | `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b` |
| Original frozen executable | 18,012,960 | `72dae29c00afdf725ead3a24a1b9826e48f02314803ed4a38094bb5d4d66ab6b` |

Environment records these exact identities and paths for every process. The final executable equals the reserve-frozen executable byte-for-byte by hash. The copied source receipt also equals the formal reserve receipt byte-for-byte, SHA-256 `03a4404a1fa9ed1f38a8aad04e3cef4a64e4f984b0d9ff7bb55d758f6ea262f6`; its original binary path points to the same frozen bytes, while the confirmation executes the copied final path. All 324 source/configuration hashes in this receipt match immutable 0e441c8, and the campaign's candidate_sources exactly match the receipt.

`B/baseline.json` identifies ea4161e and exact executable 72dae29c. Its referenced historical layout-traversal source receipt records the same original binary hash; independently all 325 recorded source/configuration inputs match immutable ea4161e. That older receipt's base_commit names fa5fb1e, the pre-change source parent; it is not relabeled as ea4161e. The recorded source content and immutable ea4161e match, which binds the later baseline metadata to the actual original measured source. Historical receipt SHA-256 is `38f11bb602f0eae9e34b4c22b32f26a5c252f9a9651c96e92ae5aec481225a18`.

This review does not rerun the reserve qualification or manufacture a new qualification receipt. The completed twelve-gate/work/source audit is separately frozen in `/tmp/projection-stack-reserve-evidence-review.md`, SHA-256 `44c6f892b4d831bd46eb3283a7cd9b0b8aa5e92e5e78084175d8e85d9c53d3ab`. The present report audits the new final-versus-original comparison itself.

`B/compare_final_original.py` is exactly `B/compare_frozen.py` with one output-directory expression changed to `against-original` for the baseline reference. No runner, sample count, warmup, cell selection, measurement, statistic or state comparison is changed. Both scripts were read as text only; neither was executed or imported. The checked command arguments run only the frozen ignored docking_cost harness, with one test thread.

The timed `measure` function is byte-identical between ea4161e and 0e441c8. Its start precedes capture_request; it then records checked apply_request, fresh project_all_roots and independent owned export_state, and ends after black-boxing the words/report. The exported-minus-start interval is the complete operation. All four recorded phase times sum exactly to that complete interval in every raw observation. Fixture construction and subsequent correctness assertions remain outside the interval. This retains the original complete-operation timing boundary rather than measuring only the newly changed stack or layout phase.

The process schedule is two serial groups, ABBA then BAAB: group 1 baseline/candidate/candidate/baseline; group 2 candidate/baseline/baseline/candidate. Every process has the same 21 workloads, 48 numbered measured samples/workload and eight configured warmups. Each process CSV has exactly 1008 rows, totaling 8064; each condition/group/workload therefore has 96 observations. All eight process return codes are zero, stderr files are empty, and each stdout records its single harness test passing. The recorded time intervals do not overlap. The campaign runs from `2026-11-07T05:48:47.295068+00:00` to `05:48:55.794199+00:00`; original-confirmation.json is recorded after measurement at 05:50:34.733522.

Each process CSV exactly reconstructs its portion of aggregate raw.csv in order. Independent checks cover the 21-cell matrix (16/128/1024 panes; one/four views; resize/center/edge and four-view cross move), sample ordinals 0–47, product identity, widths alternating 16380/16444 for resize, height 16380, all phase sums, and every node/leaf/split/item/root count. No row is dropped or inferred from a median.

All 168 complete-vector files were parsed as full preorder forests. Independent checks validate root ordering and binary child paths, complete parent/child geometry and four-unit gaps, tab selection membership, every item ID 1 through 2N exactly once, and command-specific structural cardinalities. The vectors are byte-identical across all eight processes for each of the 21 workloads. Their recomputed FNV-1a over little-endian u64 values matches every process's sample-zero digest. Across all 8064 rows, all 27 workload/width digest classes agree. Complete files are sample-zero outputs; alternate resize widths additionally have matching per-observation digests and metadata, not a separate claimed full dump for every sample.

All 84 summary rows were reconstructed for all five phases. Nearest-rank p50 uses sorted index 47, p95 index 91 and maximum index 95 of 96 observations. Every complete-operation value and signed percentage in all 42 comparison rows matches the corresponding independently computed raw statistic. All counts and range extrema exactly match original-confirmation.json:

| Complete metric | Improve | Regress | Tie | Minimum signed reduction | Maximum signed reduction |
| --- | ---: | ---: | ---: | ---: | ---: |
| p50 | 42 | 0 | 0 | +78.8192011212% | +96.0641550445% |
| p95 | 42 | 0 | 0 | +78.1468531469% | +96.4669759641% |
| max | 41 | 1 | 0 | -34.4375193692% | +97.8439124586% |

The smallest p50/p95 improvements are group 1, 16 panes/four views/resize: 5708 to 1209 ns and 6292 to 1375 ns. The largest p50/p95/maximum improvements are group 2, 1024 panes/one view/center: 1891792 to 74458 ns; 2365792 to 83584 ns; and 5689333 to 122667 ns. These ranges are independent cell statistics, not summed phase reductions or a predicted product-wide percentage.

Every complete comparison is retained below. Reduction is `100*(1-final/original)`; negative means the final runtime is slower.

| Group | Panes | Views | Workload | P50 reduction % | P95 reduction % | Max reduction % |
| --- | ---: | ---: | --- | ---: | ---: | ---: |
| 1 | 16 | 1 | resize | +79.986757 | +84.800000 | +96.510940 |
| 1 | 16 | 1 | center_move | +89.502762 | +84.994446 | +45.898018 |
| 1 | 16 | 1 | edge_split | +87.461386 | +87.609365 | +82.190295 |
| 1 | 16 | 4 | resize | +78.819201 | +78.146853 | +77.933614 |
| 1 | 16 | 4 | center_move | +90.189398 | +87.830688 | +91.440780 |
| 1 | 16 | 4 | edge_split | +86.482489 | +86.143158 | +95.847689 |
| 1 | 16 | 4 | cross_view_move | +90.311247 | +90.092034 | +89.333333 |
| 1 | 128 | 1 | resize | +82.972601 | +84.783932 | +93.476703 |
| 1 | 128 | 1 | center_move | +94.711602 | +95.013067 | +95.098308 |
| 1 | 128 | 1 | edge_split | +92.312059 | +93.281627 | +92.479785 |
| 1 | 128 | 4 | resize | +81.599414 | +83.212165 | +88.499782 |
| 1 | 128 | 4 | center_move | +94.080469 | +94.389548 | +95.305720 |
| 1 | 128 | 4 | edge_split | +93.091913 | +93.254897 | +95.319149 |
| 1 | 128 | 4 | cross_view_move | +94.288526 | +94.132653 | +74.243854 |
| 1 | 1024 | 1 | resize | +86.827127 | +87.764462 | +77.742730 |
| 1 | 1024 | 1 | center_move | +96.048580 | +95.543478 | +92.216882 |
| 1 | 1024 | 1 | edge_split | +94.255463 | +94.083490 | +88.849202 |
| 1 | 1024 | 4 | resize | +83.866418 | +86.202451 | +65.054790 |
| 1 | 1024 | 4 | center_move | +95.503227 | +95.723805 | +94.819271 |
| 1 | 1024 | 4 | edge_split | +94.428885 | +94.494149 | +89.800463 |
| 1 | 1024 | 4 | cross_view_move | +95.411976 | +95.547073 | +90.133851 |
| 2 | 16 | 1 | resize | +80.815387 | +80.241414 | +84.184615 |
| 2 | 16 | 1 | center_move | +91.208791 | +87.872748 | +84.137056 |
| 2 | 16 | 1 | edge_split | +88.392857 | +88.515552 | +87.769784 |
| 2 | 16 | 4 | resize | +79.724740 | +80.491731 | +91.540299 |
| 2 | 16 | 4 | center_move | +90.474534 | +90.218306 | +89.633583 |
| 2 | 16 | 4 | edge_split | +86.165441 | +79.865990 | -34.437519 |
| 2 | 16 | 4 | cross_view_move | +90.676144 | +94.647336 | +97.598594 |
| 2 | 128 | 1 | resize | +82.622295 | +83.600460 | +65.397118 |
| 2 | 128 | 1 | center_move | +94.819589 | +94.566262 | +95.634560 |
| 2 | 128 | 1 | edge_split | +92.914017 | +92.080020 | +92.015165 |
| 2 | 128 | 4 | resize | +82.079846 | +82.376507 | +93.166876 |
| 2 | 128 | 4 | center_move | +94.146726 | +93.542819 | +88.924792 |
| 2 | 128 | 4 | edge_split | +92.918912 | +92.363242 | +73.592863 |
| 2 | 128 | 4 | cross_view_move | +93.866421 | +94.261108 | +95.310142 |
| 2 | 1024 | 1 | resize | +86.367548 | +88.667238 | +90.376007 |
| 2 | 1024 | 1 | center_move | +96.064155 | +96.466976 | +97.843912 |
| 2 | 1024 | 1 | edge_split | +94.302301 | +93.893171 | +83.586246 |
| 2 | 1024 | 4 | resize | +83.071127 | +82.532916 | +66.793486 |
| 2 | 1024 | 4 | center_move | +95.187042 | +95.252110 | +93.352831 |
| 2 | 1024 | 4 | edge_split | +94.590404 | +94.351097 | +91.615196 |
| 2 | 1024 | 4 | cross_view_move | +95.393193 | +96.074928 | +94.242709 |

The single adverse maximum is not suppressed by a claim about improved medians or about the older confirmation. In group 2, small four-view edge, original maximum 29041 ns occurs at baseline run 2/sample 34; the other original process maximum is 24583 ns. Candidate run 1 maximum is 4250 ns, while candidate run 4/sample 25 is 39042 ns. That retained sample comprises request 1000, mutation 11125, layout 25459 and export 1458 ns, summing to 39042. This establishes the observed tail result, not an allocator/compiler attribution, a reason to remove it or evidence of a deterministic regression magnitude. The same cell still has improved p50/p95, which does not negate its maximum regression.

The archive was read in memory, never extracted or executed. Its 202 unique safe regular members contain 201 inputs and manifest.json. All member sizes/hashes agree with the manifest and roundtrip to their declared originals. This independently verifies 168 vector members, the 8064 raw observations, both scripts, baseline metadata, receipt and comparison/confirmation records. The adjacent archive-check.json count and size claims match. The archive omits executable and full source blobs; their recorded identities were separately verified against frozen executable files and immutable Git as described above. These are hash-bound evidence files awaiting their final publication commit, not a claim that the current live workspace defines the measured runtime.

| Evidence identity | SHA-256 |
| --- | --- |
| evidence.tar.xz, 163,768 bytes | `6ca3d75746bb56215c304363859116c5797c998223e36e9b9c1b72ff941e83d9` |
| Internal manifest, 201 inputs | `c33c0890c700cd417335b72ae6bd92ba0f4b5a491a3f637269def20a05f8fdcc` |
| environment.json | `0b7e82757c394ffddba60c48cf9f7afb4855417c51f9c9f9e0ae1ca8cccaf8ab` |
| raw.csv | `7956f034f97c099f1b84e597deed544b415a97b471bf98f6a2a976e2e39ff872` |
| summary.csv | `c38bca8f3aa5f8c97eebe5c854bb3152190c5bdb9eae813c3517ca51a1f4ae6a` |
| original-comparisons.csv | `85fe2ee0dc5889e729fcc5d0449c5b81b1f0cff1bcc53699e30f4fe4f47e68c0` |
| original-confirmation.json | `ad64ec19310f080a667f696b34a042b6927492d38224f2b374eb4cac34ba9996` |
| archive-check.json | `13b7e5bd2b2d329bc02ded6eb23499a9d9f84650f8bdd9e440758405cf1f2be2` |
| baseline.json | `aaa26a9faf82bf1c77cb9f73a3f37bd5dc536caa3371ec8a2ed42caafce93953` |
| compare_final_original.py | `8f19357e04760ba3753650b5ae97f16c984967cf33281bdeaf896ad4cfac3bd4` |

The confirmation metadata explicitly records no held-out workload: this is the same declared matrix, not a new unseen benchmark or an independent machine. It records that retention was not changed. This comparison demonstrates the final runtime's observed improvement over the original baseline; it does not attribute the cumulative gain to reserve alone, replace the separate actual-control/ImGui experiment, or establish all-tail superiority. The final target remains the separately audited 30/42 ImGui inventory.

Only this report was written for this confirmation audit. No Cargo/build/test/formatting command, workload, timer, archived runner, candidate script, profiler, network operation or subagent was executed. The previously frozen reserve evidence report remains unchanged.
