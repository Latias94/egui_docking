# RootStructure checkpoint evidence review

Verdict: PASS. No blocking evidence issue was found for checkpoint `0b8ab8e2ab7908af9fce477b320dff221e2aae92`. The published report accurately represents the measured candidate, actual control, complete workload, qualification results, and regressions. This accepts the evidence for this checkpoint; the twofold performance objective remains incomplete.

Reviewed on 2026-11-07. This lane only read source, Git objects, existing logs, CSVs, frozen executables, and the archive. It independently recomputed hashes, cardinalities, distributions and normalized-output comparisons in Python. It did not run Cargo, compilation, tests, a benchmark, or any archived execution script. Only this review file was written. The runtime candidate was originally authored by this lane; this is a separate evidence audit, not an independent implementation-author review. The archived implementation review by the other lane remains the independent source review.

## Control, candidate and source composition

The actual control is `80403cb4bfe66f65a48da490a2aae660091f5f06` (cold-effects), not an older prepared-report or copy checkpoint. All 17 archived pre-existing control files equal the corresponding Git object bytes. The eighteenth changed source, `graph/root_structure.rs`, is new. The frozen control source receipt binds the control files.

The measured candidate combines the original 16-path RootStructure candidate, the three-file successful-build counter addition, and one test-only `Arc::clone` qualification correction. Original candidate files differ only where the declared additions require it: `graph/root_structure.rs` gains the successful-build hook and `workspace.rs` qualifies `std::sync::Arc::clone`. The counter patch also changes `drop_resolver.rs` and `docking_cost.rs`, giving 18 changed source files in total. The missing-Arc failure and subsequent corrected qualification are both archived. No assertion was removed by that compile correction.

The original patch SHA-256 is `8dc1059d026310e0ef26a9ff5bcb1bca92cd8d00cff62a0bcf833a0c1404ad0b`. Its original array-order base/candidate set hashes reproduce as `94fbc705a65eab3df64b024ef63d7760cd1a8d40299777c5d3310fd67d142b6f` / `4b6096fc6b411e16479afacec2144f3348613b9c80bfdfc63d896f3b04b1b827`. The original description incorrectly calls that order lexical; the checkpoint report explicitly corrects the description and retains the frozen receipt. The per-file hashes and independent formal build receipt remove any ambiguity. This is resolved documentation, not a blocker.

The formal build receipt records 324 source hashes, `DOCKSPACE_BENCH_NO_WORK_COUNTERS=1`, a successful production-profile build and unchanged sources. The counters-enabled qualification receipt has 323 source hashes; all overlap exactly with the formal receipt. All 18 archived measured source files match their formal receipt hashes. The original measured source read also matched the full receipt. Subsequent NodeResult/MRU work is outside this review; this verdict binds the archived candidate rather than any later workspace state.

The following executables were read and independently hashed, without execution:

| Executable | SHA-256 |
| --- | --- |
| RootStructure candidate | `d0facb66482a23377bcdaad5e3bb5be04de9fe6ee35f521a51d78e24cfdf9ba2` |
| Actual cold-effects control | `daf9156d7f2b87612f05abd3c364097d06e09c42dc8a32a1c352b4206a759a21` |
| Pinned ImGui | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

The ImGui source pin is `d88f238d1d363ef387382b2e29ad12cf96f826ac`. The executable hashes match both pairing receipts. The archive stores source/build/run provenance; executable files themselves remain in the frozen temporary paths.

## Archive integrity and qualification

The archive is 511,412 bytes. Reading every regular member yields 467 unique paths: 466 input files and `manifest.json`. The regular-member payload total is 16,797,032 bytes. Every input byte length and SHA-256 matches the manifest. All archived inputs also match their original input bytes or actual-control Git object bytes; the six published SHA256SUMS entries agree. The manifest SHA-256 is `2e73399893bf8b70d7aff78fb6a6b382f5c8105d5c40c29d5dea234f13e61d58`. These checks independently support the published archive-check receipt.

The recorded normal, counters-enabled qualification logs substantiate 408 focused tests, four work regressions, 2,064 production-core tests, 2,240 workspace tests, four no-default public integration tests, and 292 / 335 public host tests. The complete core/workspace suites each skip the same three opt-in probes. All recorded formatting, correctness/suspicious Clippy, strict-docs, doctest and dependency checks return success. Both registered 10,000-depth layout tests appear as passing, including the behavior_tests registration; these are not merely unregistered legacy files. The initial missing-Arc compile failure is retained and correctly described as a compile correction, not a runtime failure.

These are reviewed execution receipts from root's serial runs, not tests rerun by this lane. The approximate 0.007-second recovery-test log is correctly presented as a coarse observation rather than a paired speedup claim.

## Workload, ordering and complete outputs

Each of the two pairings independently covers the original 21 cells: total panes 16 / 128 / 1024; one / four views; resize, center move and edge split at both view counts; cross-view center move at four views. Group 1 is ABBA; group 2 is BAAB. Each process supplies eight warmups followed by 48 formal samples per cell. Each group/condition/cell therefore has 96 observations. All 16 process receipts return zero and their start/finish intervals do not overlap.

Every process CSV has exactly 1,008 unique rows and sample IDs 0 through 47 per cell. Concatenating the process CSVs reproduces each combined raw CSV exactly: 8,064 rows per pairing, 16,128 total. No sample or tail was discarded. All node/leaf/split/item/root cardinalities agree with the fixture contract, including the extra leaf and split for edge. Resize alternates the two prescribed widths; all other geometry dimensions match the protocol.

Every raw row has nonnegative phases and a positive complete-operation time. In every row, the four recorded phase durations sum exactly to the independently recorded complete-operation duration. This verifies the recorded schema and nesting, without treating sums of phase medians as complete-operation medians.

There are 336 complete sample-zero vectors: 16 processes times 21 cells. For each cell all 16 vector files are byte-identical across both Rust conditions and pinned ImGui. Their full words reproduce their raw sample-zero FNV-1a digests. Independently parsed vectors have exactly `9 * nodes + 2 * panes` words, correct root identities and binary DFS paths, ordered node records, complete unique item inventory, and a selected intent that belongs to its leaf. Every raw digest also agrees for the same cell and width across all processes and products. Full-vector identity therefore does not rest only on equal digests. The exported selection contract is the protocol's command intent, not a claim about a rendered tab.

## Numeric results and all regressions

I recomputed all 2,520 summary statistics from the raw data: two pairings times 84 summary rows times five phases times p50/p95/max. The existing nearest-rank rule is preserved: with 96 observations, indices 47, 91 and 95 after sorting. Every statistic matches its stored summary. All 84 comparison rows, including both baseline/candidate values, ratios and percentage reductions for p50/p95/max, then reproduce from those summaries.

Against actual `80403cb`:

| Complete-operation statistic | Improvements | Ties | Regressions | Reduction range |
| --- | ---: | ---: | ---: | --- |
| p50 | 36 | 1 | 5 | -15.48% to +45.23% |
| p95 | 36 | 0 | 6 | -46.50% to +48.30% |
| max | 27 | 1 | 14 | -221.30% to +93.31% |

Every reported 1024-pane range reproduces: resize/center/cross improve 24.96–30.91%; four-view edge improves 14.28–16.23%; one-view edge regresses 10.84–15.48%. The smaller one-view edge cases range from an exact tie to a 4.29% regression.

For completeness, the following includes every control group/cell with a negative reduction in any complete-operation statistic. Positive entries in this table are retained to avoid implying all three statistics regress together.

| Group | Panes | Views | Case | p50 reduction | p95 reduction | max reduction |
| --- | ---: | ---: | --- | ---: | ---: | ---: |
| 1 | 16 | 1 | edge_split | -3.85% | -0.85% | -0.02% |
| 1 | 128 | 1 | center_move | +18.06% | +17.17% | -23.94% |
| 1 | 128 | 1 | edge_split | -4.29% | -15.65% | -42.98% |
| 1 | 128 | 4 | cross_view_move | +27.56% | +24.05% | -64.08% |
| 1 | 1024 | 1 | edge_split | -15.48% | -25.82% | -16.14% |
| 2 | 16 | 1 | resize | +40.64% | +38.64% | -28.85% |
| 2 | 16 | 4 | edge_split | +23.81% | +11.19% | -26.67% |
| 2 | 128 | 1 | center_move | +24.23% | +11.68% | -221.30% |
| 2 | 128 | 1 | edge_split | -0.20% | -15.44% | +53.43% |
| 2 | 128 | 4 | resize | +30.16% | +34.09% | -61.95% |
| 2 | 128 | 4 | edge_split | +20.00% | -46.50% | -106.64% |
| 2 | 128 | 4 | cross_view_move | +27.81% | +28.25% | -44.10% |
| 2 | 1024 | 1 | resize | +29.97% | +22.40% | -58.64% |
| 2 | 1024 | 1 | edge_split | -10.84% | -11.97% | -35.70% |
| 2 | 1024 | 4 | resize | +30.91% | +30.61% | -44.73% |

The report's group-1, 1024-pane, one-view edge example is exact: layout p50 111,791 to 74,166 ns; mutation 18,791 to 79,084 ns; complete operation 145,875 to 168,458 ns. It identifies the affected measured phase and does not claim an unmeasured breakdown among body rows, placements, sorting, Arc work or other calls.

Against pinned ImGui, the p50 ratios and twofold counts also reproduce:

| Panes | Complete-operation p50 ratio range | At most 2x |
| --- | --- | --- |
| 16 | 2.60–5.47x | 0 / 14 |
| 128 | 1.44–2.27x | 7 / 14 |
| 1024 | 1.18–1.58x | 14 / 14 |

All 42 p50 and all 42 p95 ratios are greater than one; 27 max ratios are greater than one. Thus 21 / 42 group/cells meet the p50 twofold threshold. The report correctly keeps the incomplete goal and the single-view edge regression visible. It makes no uniform-win or exact cost-attribution claim.

## Timers and successful-build counters

The actual-control to candidate diff in `docking_cost.rs` changes only the ordinary work CSV header, format and three appended fields. The formal timing function, capture/apply/layout/export boundaries, fresh fixture per iteration, sample/warmup handling, full state validation and destruction/lifetime assertions are unchanged. Request capture, checked mutation, fresh constraints collected for every actual leaf, all-root numeric projection and fresh complete owned export remain in the existing complete-operation timer. `PreparedTransaction::publish` replaces and drops the previous workspace before checked mutation returns. New body completion occurs in the actual checked mutation. Fixture/Builder construction remains before the timer as in the control; it has not replaced mutation completion. Rich outputs and exported vectors outlive fixture destruction and are dropped after timing, as before.

The independent `layout/reference.rs`, override tests, comparison scripts and pinned native implementation remain byte-identical to the actual control. The layout diff changes ownership and access to actual split payloads, not the numeric solver/override algorithm or its arithmetic ordering. The raw diagnostic adapter remains separate from the graph-owned runtime path.

The work CSV preserves its original 20 columns and adds exactly three successful-output counts. The build hook appears only after `finish_directory` and before successful return from graph-owned `RootStructure::build`. It invokes the existing update path, which returns immediately under the timing bypass; its arguments are two constant-time Vec lengths. No raw adapter hook or additional traversal is introduced.

The ordinary reset occurs after fixture setup and initial projection; snapshot occurs after the measured operation and before validation/destruction. Each of the six edge cells completes one affected root:

| Panes | Views | Successful builds | Node rows | Shared split handles |
| --- | ---: | ---: | ---: | ---: |
| 16 | 1 | 1 | 33 | 16 |
| 16 | 4 | 1 | 9 | 4 |
| 128 | 1 | 1 | 257 | 128 |
| 128 | 4 | 1 | 65 | 32 |
| 1024 | 1 | 1 | 2049 | 1024 |
| 1024 | 4 | 1 | 513 | 256 |

All other 15 cells have zero successful builds, rows and handles. The total is **6 builds, 2,926 rows and 1,460 split handles**, not 12 builds. The row/handle formulas are exactly `2 * (panes / views) + 1` and `panes / views`. These count constructed body outputs; they do not count all graph visits, allocation events, capacity, placement updates, failed partial builds or command commits. A successful build later rolled back still counts. Zero bodies/fingerprints does not prove zero walks. The published report states these limits accurately.

## Exact artifact binding

All paths below are the files named in this review's scope; temporary receipt paths are under `/tmp/dockspace-imgui-twofold-20261107-ea4161e/`.

| Artifact | SHA-256 |
| --- | --- |
| Report | `e550ac5e534ca40f9d8cc5267201e545481b6048d09aa5bccb6495ecc1777f73` |
| All comparisons | `5b66348d759207a16e0747a8326b5e664bc781061fd4a4c1fab2e022248b652e` |
| Evidence archive | `9db24f42bc0431113139c4902fe37b257d5df248d547aa834c3820bd8f56dd6d` |
| Published checks | `fda897383291f36200e37855430ab976933e8a56b8deb4f8834119732d3a9546` |
| Published archive check | `c400cf7d6e1532b5d1eecd9bafdbe06c7da1c8499286e26cb49cdc582b9a23b8` |
| Formal source/build receipt | `89df7e9834261ddb454b85796daec4cb8fa897306bbfc500ba1307fc27870944` |
| Complete qualification receipt | `b227f2aa30a93d6a54c8cddbe75d8195fe58de0f60cbebdad437cb5711615094` |
| Frozen tracked runtime diff | `8cc2cd476fd9f4ba3546f4704c0c586ca0bd18f7a975911067a17bc05fe92d1c` |
| Work augmentation patch | `4587fcc923b7cbbf79f8c74c1a96a2eae6dd7df52c770f991167fb160c646240` |
| Ordinary work CSV | `d06e5bba0bf4bc900e6c65ab7f960bde73092551c380fd93a2cddcff03f26637` |
| Control raw | `118e61ce4c6b8ff9df08fffd58fd0fe442f250ce3f0e92e11784c5cbc9c1530f` |
| Control summary | `0d512f4472ccaae8c9373295755a79345f6051c2a41a9d883198924d337cdd4c` |
| ImGui raw | `1ca3eebb8c6bc29d4019a561a8155bc9eb4f2f9070c622e83215dfee7d87e704` |
| ImGui summary | `8d0a552532e4d4433985d899e3aa912ebb457c449b6264445c48ded7ec6e8626` |

The exact archived measured source hashes follow. Paths are relative to `crates/dockspace/src/`.

| Source | SHA-256 |
| --- | --- |
| `canonical.rs` | `d6e41e8a7145f0354119e59652ecf050f8321240a0504fe2d4e45d2575629ae2` |
| `docking_cost.rs` | `391de3001bd30c9e99936d244ddf31bce7d50e429922f026ce058dd4f43a989b` |
| `drop_resolver.rs` | `3845cf1c9c247c79444d310969c119c53a76f5ae5ab228c260627b0d1371540e` |
| `engine/presentation_authority.rs` | `5582175d3a88b4703d04cb927a979811b12d7dbe8c4d2682a9f7d57e80210a25` |
| `graph.rs` | `88dac213032ad21cd3d5fec882ebbd050743c9d3147b66a3abfe5bae19fc7ee0` |
| `graph/authority.rs` | `c4542993266ad497b50cdd30e00bbf6fc656819925d8d52942290b54d7d121e2` |
| `graph/root_structure.rs` | `de30fe16e4587efe9cc181f5d1ca46b66250fa082c5b4363efa5f1c9a1215a62` |
| `graph/shared_storage.rs` | `706d33c0d155937478907783028123b5e8a88ec7aec4a2f93458ad306cbe85cf` |
| `layout.rs` | `aefd20cd30f4c77b70a60971b82708882ef5e76fe6195e08e01c782e65e03903` |
| `layout/projection.rs` | `f629408b664a3c32663098860a89f17d4830455861903d869f549b739c9d2ec1` |
| `layout/topology.rs` | `d2d06077b9a5bfb03f764b0ed9c862de4e9aca3a65946059e480b8412f356f56` |
| `layout/topology_tests.rs` | `fbb46bcf4d970f4ed61a17bd24e605b31e71c61b80b2988bf3462d5e7f9c988a` |
| `operation.rs` | `47fc2dd6018c0286039cc2f6eb25ddb88b875682b528d0dc467a634560260eaf` |
| `operation/tests/owner_reference.rs` | `99e0e96521bc91aab550fb66e281a4ac5cf2599060d11e6832b1fd6e23349768` |
| `operation/tests/split_resize.rs` | `8d0e5d539b99c1faa51fadd25ff7ae1f81fa72dd285ccc8552afefb0f7a35ec1` |
| `runtime/error_kind.rs` | `b13d22a481eb5205750ca951b3eafd25c9716e9a808a033d9ca9719a56caf7c8` |
| `scene_compiler/surface.rs` | `9acd698a293a8e8406f3f6c4f3cac9d0227b7a84f89c048b46a1c546ea66e8e6` |
| `workspace.rs` | `15f785fee41d35cd25508d14d847f472000ccdfa4ece2012c534c92759a231f3` |

No evidence correction is required before retaining this checkpoint. Subsequent NodeResult/MRU candidates and any new measurements require their own source/receipt binding.
