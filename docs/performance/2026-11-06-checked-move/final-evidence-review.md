# Final checked-move v2: independent evidence review

Reviewed at 2026-11-06T15:10:46.919480+00:00. Evidence root: `/tmp/dockspace-checked-move-20261106-a2bcabc`. Baseline label: `a2bcabc1440caa664fb5f4a64c4669c4eecfdce4`. Candidate source is the frozen, independently reviewed v2; the first iteration's failed-gate evidence remains separate.

**Verdict: no blocking data-integrity or arithmetic finding. The formal center/edge/cross improvement criteria pass.** At 1,024 total panes, one-view center improves **43.915306% / 42.787024%**, and four-view center **27.965679% / 27.050980%**, in the two direct baseline/candidate groups. All move cells at every declared size improve in both groups. Formal resize has small repeated negative p50 cells and mixed negative tails; the root has scheduled a separate frozen-binary resize follow-up to assess that residual question. The completed follow-up is independently reviewed in the addendum below and must not replace, remove or be pooled into the formal cells. Full task/verification completion is not inferred from these performance data alone.

This reviewer only read source receipts, existing binary/text/CSV bytes and performed independent arithmetic, schema/digest and hash checks. No Cargo, compiler, test, benchmark, executable, profiler, network, Git write or nested agent was run. The only write is this report. Root-owned integration/Clippy checks continue separately and are not represented as complete in this verdict.

## Samples, durations, summaries and full state

Both `frozen-comparison-v2` and `candidate-reference-v2` independently contain exactly **8,064 raw observations**, eight processes with 21 cells × 48 measured samples each. Each group/condition/cell has **96** observations; each pooled condition/cell has **192**. There are no missing/duplicate sample identities, unexpected cells or dropped tail rows. The aggregate is the exact ordered concatenation of the original process CSVs; ImGui stdout equals its raw CSV bytes.

All **16,128 observations** have nonnegative request/mutation/layout/export times, a positive direct total, and exact integer equality `request + mutation + layout + export == operation_owned`. Counts and actual bounds agree with the protocol: 16/128/1,024 total panes divided over one/four roots, two initial tabs per pane, edge adding two nodes/one leaf/one split, no item loss/duplication, resize widths 16,380/16,444 with height 16,380 and correct eight-warmup parity.

Each dataset's **84 group summary rows** independently recompute with nearest rank (`sorted(values)[ceil(n*p/100)-1]`), including all five p50/p95/max metrics and count 96. That is **2,520 checked percentile entries across both datasets**. Each new `pooled-attribution.csv` has exactly 42 rows with count 192. All **1,260 pooled percentile entries and 840 duration-sum/share entries** recompute from the raw samples. Pooled quantiles are actual raw-operation statistics, not percentiles pooled from process summaries. Ratios and reductions below compare separate p50 estimates, not paired ratios or averages of group medians.

There are exactly **21 sample-zero dump files per process**, **168 per dataset / 336 total**. Every corresponding full vector is byte-identical across baseline, final candidate and pinned ImGui. Independent parsing of all 21 unique vectors verifies decimal u64 words, nine-word node prefixes, binary preorder/path identities, root ordinals, axis kinds, exact nonnegative integer rectangles, separator-4 half partitions, complete unique ordered members, selected membership and moved-item destination/intent. Every vector has `9*actual_nodes + 2*initial_panes` words. Independent FNV1a64 over eight little-endian bytes per word matches sample-zero digests.

All **27** distinct case/panes/views/width states have the same digest across processes and datasets. Both final output vectors and raw state digests also match the retained first-iteration baseline values. Sample-zero resize dumps contain width 16,380. The second resize width is covered by all corresponding raw digests and the probes' outside-timer full-state oracle, not a separately archived full vector. Complete agreement therefore means corresponding archived sample-zero vectors plus all-sample digest/oracle checks, not 16,128 independently archived complete dumps.

## All 21 formal before/after cells

Times are **microseconds**, baseline → candidate. Positive reduction means less candidate time. Every group value pools 96 actual observations from two processes.

| Panes | Views | Case | Group 1 before → after | Reduction | Group 2 before → after | Reduction |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 16 | 1 | `resize` | 7.125 → 6.583 | 7.607018% | 6.625 → 6.875 | -3.773585% |
| 16 | 1 | `center_move` | 45.791 → 24.333 | 46.860737% | 45.542 → 24.625 | 45.929033% |
| 16 | 1 | `edge_split` | 49.041 → 27.791 | 43.331090% | 52.208 → 28.000 | 46.368373% |
| 16 | 4 | `resize` | 6.750 → 7.041 | -4.311111% | 6.833 → 6.875 | -0.614664% |
| 16 | 4 | `center_move` | 30.333 → 21.791 | 28.160749% | 31.125 → 21.833 | 29.853815% |
| 16 | 4 | `edge_split` | 31.958 → 23.417 | 26.725702% | 32.416 → 23.583 | 27.248889% |
| 16 | 4 | `cross_view_move` | 29.500 → 22.458 | 23.871186% | 30.208 → 22.500 | 25.516419% |
| 128 | 1 | `resize` | 57.084 → 57.250 | -0.290800% | 57.583 → 56.916 | 1.158328% |
| 128 | 1 | `center_move` | 372.875 → 194.417 | 47.860007% | 370.583 → 194.333 | 47.560196% |
| 128 | 1 | `edge_split` | 394.041 → 218.875 | 44.453750% | 399.542 → 216.292 | 45.865015% |
| 128 | 4 | `resize` | 54.958 → 55.666 | -1.288256% | 55.083 → 54.959 | 0.225115% |
| 128 | 4 | `center_move` | 253.459 → 177.500 | 29.968950% | 253.833 → 177.125 | 30.219869% |
| 128 | 4 | `edge_split` | 262.542 → 184.292 | 29.804755% | 259.416 → 182.708 | 29.569495% |
| 128 | 4 | `cross_view_move` | 255.000 → 181.541 | 28.807451% | 254.250 → 184.208 | 27.548476% |
| 1024 | 1 | `resize` | 581.000 → 594.625 | -2.345095% | 596.500 → 592.208 | 0.719531% |
| 1024 | 1 | `center_move` | 3483.125 → 1953.500 | 43.915306% | 3471.459 → 1986.125 | 42.787024% |
| 1024 | 1 | `edge_split` | 3668.666 → 2158.417 | 41.166162% | 3671.375 → 2176.667 | 40.712485% |
| 1024 | 4 | `resize` | 471.125 → 472.000 | -0.185726% | 472.375 → 475.333 | -0.626197% |
| 1024 | 4 | `center_move` | 2328.000 → 1676.959 | 27.965679% | 2335.250 → 1703.542 | 27.050980% |
| 1024 | 4 | `edge_split` | 2386.166 → 1735.333 | 27.275261% | 2399.208 → 1751.833 | 26.982863% |
| 1024 | 4 | `cross_view_move` | 2352.875 → 1749.125 | 25.660097% | 2359.959 → 1768.625 | 25.056961% |

The main 20% plan/goal gate passes for both center shapes in both groups; the smallest center margin is four-view group 2 at 27.050980%, about 7.05 percentage points above the criterion. This threshold is the plan's investment criterion, not a user-supplied number. Edge/cross direction agrees at every declared size, with no changed-state p50 regression in either group.

## Resize and negative tails remain visible

The formal result is not an all-cell speedup. The 16-pane/four-view resize cells are slower by **4.311111% and 0.614664%**, absolute **0.291 us and 0.042 us**. At 1,024 panes/four views they are slower by **0.185726% and 0.626197%**, absolute **0.875 us and 2.958 us**. One-view resize changes direction between groups at every size, including 1,024 panes (**2.345095% slower / 0.719531% faster**). Four-view/128 also changes direction. These are nominal repeated negative directions at two four-view sizes; it would be inaccurate to say every cell improves or that resize is unchanged.

The repeated negative magnitudes are small/inconsistent between groups, with no consistently large p50 slowdown in this matrix. That observation is not a statistical proof of equivalence or a causal explanation. The formal data alone should not be used to label every negative estimate “noise.” The separately planned 96-sample/16-warmup resize-only control should be reviewed as additional evidence, without replacing these original formal rows. There is no new resize optimization claim.

Pooled 1,024-pane tails (192 observations per condition/cell), pairs baseline / candidate:

| Views | Case | Pooled p50 pair | Reduction | P95 pair | Maximum pair |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | `resize` | 588.583 / 593.208 | -0.785786% | 690.083 / 736.583 | 773.792 / 1399.042 |
| 1 | `center_move` | 3478.833 / 1972.208 | 43.308345% | 3892.334 / 2208.958 | 6361.000 / 2468.458 |
| 1 | `edge_split` | 3670.292 / 2171.250 | 40.842581% | 4079.375 / 2361.167 | 4433.000 / 2534.208 |
| 4 | `resize` | 472.375 / 473.167 | -0.167663% | 532.875 / 556.083 | 782.667 / 839.458 |
| 4 | `center_move` | 2333.500 / 1695.375 | 27.346261% | 2606.000 / 1949.958 | 3080.459 / 2099.667 |
| 4 | `edge_split` | 2390.667 / 1748.291 | 26.870158% | 2719.500 / 1928.167 | 2988.250 / 2182.250 |
| 4 | `cross_view_move` | 2354.458 / 1755.500 | 25.439316% | 2555.916 / 1980.167 | 2884.375 / 2434.542 |

All changed-state 1,024-pane p95s and maxima improve in both formal groups. Across the **entire** matrix, however, there are seven negative group-p95 cells and eight negative group-maximum cells. Every such tail cell is listed below to preserve small-size and resize counterexamples:

| Group | Panes | Views | Case | Statistic | Baseline / candidate, us | Increase |
| --- | --- | --- | --- | --- | ---: | ---: |
| 1 | 16 | 4 | `resize` | p95 | 7.042 / 7.791 | +10.636183% |
| 1 | 128 | 1 | `resize` | p95 | 69.042 / 70.333 | +1.869876% |
| 1 | 128 | 4 | `resize` | p95 | 60.041 / 61.500 | +2.430006% |
| 1 | 1024 | 1 | `resize` | p95 | 720.958 / 766.333 | +6.293709% |
| 1 | 1024 | 4 | `resize` | p95 | 513.667 / 552.083 | +7.478775% |
| 2 | 128 | 4 | `resize` | p95 | 59.000 / 60.125 | +1.906780% |
| 2 | 1024 | 4 | `resize` | p95 | 561.500 / 577.500 | +2.849510% |
| 1 | 16 | 1 | `edge_split` | maximum | 54.042 / 54.917 | +1.619111% |
| 1 | 128 | 4 | `edge_split` | maximum | 436.417 / 468.750 | +7.408740% |
| 1 | 128 | 4 | `resize` | maximum | 67.084 / 203.000 | +202.605688% |
| 1 | 1024 | 1 | `resize` | maximum | 773.792 / 1399.042 | +80.803368% |
| 2 | 16 | 4 | `resize` | maximum | 7.333 / 8.917 | +21.600982% |
| 2 | 128 | 4 | `resize` | maximum | 73.834 / 225.042 | +204.794539% |
| 2 | 1024 | 1 | `resize` | maximum | 765.958 / 933.750 | +21.906162% |
| 2 | 1024 | 4 | `resize` | maximum | 744.459 / 839.458 | +12.760810% |

Notable repeated resize maxima are 128 panes/four views: **67.084 → 203.000 us** and **73.834 → 225.042 us**. One-view/1,024 maxima also rise in both groups. The small-size changed-state counterexamples are group-1 one-view/16 edge maximum and group-1 four-view/128 edge maximum; their p50s still improve substantially. No outlier is discarded, maximum is a sample maximum rather than a guarantee, and single-desktop ABBA/BAAB does not yield a randomized multi-machine confidence interval.

## Separate candidate/ImGui context and phase shares

The final candidate/reference experiment is separate from the direct frozen improvement comparison. Each pooled value uses 192 raw observations; ratio is **candidate / ImGui**. Do not mix its samples with the before/after groups, or compare its final median directly with an older separately scheduled ImGui experiment as a controlled speedup estimate.

| Views | Case | Final candidate p50, us | ImGui p50, us | Ratio |
| --- | --- | ---: | ---: | ---: |
| 1 | `resize` | 591.834 | 70.708 | 8.370114x |
| 1 | `center_move` | 1997.833 | 72.458 | 27.572290x |
| 1 | `edge_split` | 2201.125 | 115.584 | 19.043509x |
| 4 | `resize` | 477.416 | 70.417 | 6.779840x |
| 4 | `center_move` | 1723.666 | 70.958 | 24.291355x |
| 4 | `edge_split` | 1778.375 | 110.958 | 16.027461x |
| 4 | `cross_view_move` | 1787.917 | 71.292 | 25.078789x |

For final candidate 1,024 panes/four views, directly recomputed shares are `sum(phase durations) / sum(direct total)` from the **same raw operations**:

| Case | Request | Mutation | Layout | Export |
| --- | ---: | ---: | ---: | ---: |
| `center_move` | 8.939250% | 62.575815% | 26.740624% | 1.744310% |
| `edge_split` | 8.574708% | 63.654931% | 26.087518% | 1.682843% |
| `cross_view_move` | 8.276676% | 64.239760% | 25.755582% | 1.727983% |

This supports the prose ranges mutation **62.6–64.2%**, layout **25.8–26.7%**. These are additive phase-sum shares, not median sample shares or subtractions of separate medians. Four-view center's independent phase p50s are request **151.834 us**, mutation **1,078.083 us**, layout **455.042 us**, export **28.708 us**, with direct total p50 **1,723.666 us**. The separate medians must not be added into a new total estimate.

The workload still includes actual checked publication and all-root rich layout/materialization, and both sides allocate the same fresh owned normalized output. Fixture construction, full correctness oracle, digest, IO and rich result disposal are outside timing; real transaction/replaced-workspace disposal remains inside apply. The timer excludes UI/tab rendering, fonts, scene/hit compilation, renderer/GPU, OS viewport lifecycle and host settlement. Selection is command intent, including ImGui pending selection, not rendered visibility/scroll or a full click. Equal exported success state does not imply equal failure/rollback contracts, language-level performance floors, feature parity, heap-byte/RSS savings or complete product-frame improvement.

## Work counts, source and executable ownership

`work-counts-v2.csv` is byte-identical to v1's 21-row `work-counts.csv`, and every row independently matches the declared source formula: unchanged full request captures V roots, while a same-root command captures one root and cross-view captures two. With N=2T−V and n=2(T/V)−1, builds/visits are V+1 / N+n or V+2 / N+2n. Resize's seven counters are zero.

At 1,024 panes: one-view center/edge is **2 builds / 4,094 visits**; four-view center/edge **5 / 2,555**; cross-view **6 / 3,066**. All moves retain **4 multiset scans / 8,192 item occurrences / 1 complete candidate clone**, with 2,047 or 2,044 original nodes and 2,048 tab items. These enabled-counter clocks are not formal latency data. Per-Node and short-MRU allocation removal is source evidence; neither timing nor this ledger supplies allocation event/byte totals.

All **325 recorded candidate input hashes** equal the current source tree and the frozen comparison's copied source map. The seven reviewed source hashes also equal the v2 implementation review's exact snapshot table. All frozen executable and raw/summary hashes match independently read bytes. The baseline is the same `793e...6d7` executable used by v1; v2 candidate is `fc16...ce81`; pinned ImGui remains the same `32fa...6c0`. All 10 pinned upstream input hashes match. All **49 protected user files** retain starting hashes.

The preserved v1 runner and v2 runner differ only in output directory, candidate executable/source-receipt paths. Their execution, sample settings, validation, digest, summary logic and serial order are the same. `compare_frozen-v1.py` has hash `e323...e4c`; v2 `compare_frozen.py` has `f0a0...6feb`. The first review listed the mutable `compare_frozen.py` path after its v2 path reconfiguration, so that earlier `f0a0...6feb` entry identifies the v2-configured script, not the separately preserved v1 copy. This path identity clarification does not change any v1 source, executable, CSV or reported measurement hash.

Rust artifact/profile receipts retain opt-level 3, debug assertions/overflow checks disabled, production ThinLTO/one codegen unit and compile-time structural ledger bypass. Both builds and all 16 processes across the two v2 datasets form **18 successful nonoverlapping intervals**. Candidate-reference builds finish at **15:00:39.275442 UTC** before reference timing; reference timing finishes before the direct frozen comparison begins. Formal frozen timing runs **15:01:49–15:02:05 UTC**. The receipts prove root-runner serialization, not the absence of arbitrary unrelated desktop scheduling effects.

The existing all-features log reports **2,023 tests run, 2,023 passed, 3 skipped** for v2. These are inspected root-run logs, not an independently rerun suite. Subsequent integration, Clippy and dependency checks were still root-owned ongoing work at this review. Their completion is not implied by the performance or source verdict.

## Results document review and remaining follow-up

Reviewed `docs/performance/2026-11-06-checked-move.md`, SHA-256 `b4c89d93c77c2996da0bb4a4ba1e3ba310ad58decc23ed7b00ac70ad5d326c5f`. All **21** before/after rows, all **seven** candidate/reference rows, all quoted first-iteration gate/tail values, phase p50s, phase-sum shares and stated work-count examples match the raw/source evidence and correct rounding. It correctly distinguishes the failed v1 result, groups versus pooled samples, sample-zero vectors, selected intent, logical views, residual contract differences, memory measurement limits and source/counter evidence versus isolated causal ablations. Local links resolve to the plan and earlier matched-kernel document.

No blocking wording/number error was found. One disclosure addition is recommended: the current draft reports 1,024-pane resize tails but should also mention the repeated **128-pane/four-view resize maximum increases** and optionally the group-1 128/four-view edge maximum. This report includes them fully. The upcoming resize follow-up should be labeled a separate diagnostic with its own settings/receipt; the original formal negative cells must remain visible. This section should be appended with the completed follow-up result before making an unqualified overall resize-stability claim.

## Artifact hashes at this review

| Relative artifact | SHA-256 |
| --- | --- |
| `baseline-docking-cost` | `793e8f2c166cbee6cee4885b01052d25415c9ccac06fee4189be3945de96f6d7` |
| `candidate-reference-v2/dockspace_docking_cost` | `fc16f5bc3ad205fe6b6800440c61573295df4bca1a64e6bde1097a483044ce81` |
| `candidate-reference-v2/imgui_docking_cost` | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |
| `frozen-comparison-v2/environment.json` | `566a6739f5b62636e69c12928dda83950cc0f8ffb5201298f4ce8587363e017a` |
| `frozen-comparison-v2/raw.csv` | `28995a61988ed3d758b0123902b88e673438858b961fb17e67f2ed724b9887b6` |
| `frozen-comparison-v2/summary.csv` | `3642a879bd39351b26cffbde47ba4186bc36edf68620571917663db3244deadb` |
| `frozen-comparison-v2/pooled-attribution.csv` | `d8cc2266a8e6d1496ad34a8405e0b333f5b4bda90a42cdd9f22736387bf77487` |
| `candidate-reference-v2/environment.json` | `69ee731dcd5c46324d3ca54fc9e77505f9312217246f919d585f8290e564ca9f` |
| `candidate-reference-v2/raw.csv` | `60cc33f463f9f270e2d72dd71e590087e181571f7f78786a8832bd5dd6900034` |
| `candidate-reference-v2/summary.csv` | `8c4ffb92deb4e37c39f565d78d33f5a032276d3e2ed2ac66543364fb8fab7a68` |
| `candidate-reference-v2/pooled-attribution.csv` | `b584b34edcdf190180a7413e5aa74b9189cccede3e84d04a2a594534f31c1ba8` |
| `compare_frozen-v1.py` | `e3236200d1f87949069a62aa6dd4c9283b719c93b91ab47370f73ad0005e0e4c` |
| `compare_frozen.py` | `f0a072dae76ebf6295caa33f43a8f67bab2db8955857685e0107ee4f86796feb` |
| `work-counts-v2.csv` | `d096a75896956878bd6229ddf498ad059358c31337f235900940bde9c0e31b3e` |
| `nextest-v2.log` | `27a96cfeb578bf41c0cdafb935fae17138aa7a32a193fdf4ef97a22740985467` |

The measured v2 result establishes the planned center improvements in both groups, consistent move improvements at every size and complete declared output agreement, while preserving formal resize and tail counterexamples. The completed independent resize-control review is appended below. Root's remaining verification is a separate completion item; no result has been resampled or relabeled by this reviewer.


## Completed resize-only follow-up: independent review

Audited at 2026-11-06T15:18:45.177325+00:00. This addendum closes the previously pending resize-control review. It does not replace or pool the formal negative cells, and it does not turn the qualified resize result into a claim of zero regression.

**The control is complete and internally consistent; no new measurement-integrity blocker was found. Four-view 1,024-pane median slowdown did not repeat, while a modest one-view median slowdown remains an explicit unresolved limitation.** No additional runtime change or measurement is requested by this review.

The existing `resize-followup` uses the same frozen baseline and v2 executables and the identical 325-file source map, with **96 samples / 16 warmups per process**, all six pane/view resize cells and two serial ABBA/BAAB groups. Independent checks verified **4,608 raw rows**, 576 unique expected observations per process, exact original-process/aggregate equality, and **24 group summaries** with **360** correct nearest-rank p50/p95/max entries. There are **192 raw observations per group/condition/cell**, and 384 when pooled across the two control groups. All phase sums, shape/item/root counts, dimensions and resize parity agree.

Each process has its exact six-file sample-zero dump set: **48 full vectors**. All corresponding bytes match the already parsed formal resize vectors. Recomputed sample-zero FNV digests agree; all **12** pane/view/width state digests match across both conditions/processes and the original formal data. As before, sample zero is width 16,380; width 16,444 has all-sample digest/full-probe-oracle coverage but no separately archived complete dump. Both executable hashes, every recorded source hash and all **49 protected-file hashes** still match. Eight successful control-process intervals run serially at **15:11:13.019889–15:11:19.730671 UTC**, after the formal run, with no build invocation in this control runner.

All control group results below are **microseconds**, pairs baseline / candidate; positive reduction means the candidate was faster:

| Panes | Views | Group | P50 pair | Reduction | P95 pair | Maximum pair |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 16 | 1 | 1 | 7.333 / 7.250 | 1.131870% | 9.041 / 9.250 | 10.833 / 11.917 |
| 16 | 1 | 2 | 7.250 / 7.125 | 1.724138% | 9.125 / 7.625 | 30.459 / 40.709 |
| 16 | 4 | 1 | 7.000 / 7.125 | -1.785714% | 7.250 / 7.500 | 8.041 / 39.958 |
| 16 | 4 | 2 | 7.000 / 6.959 | 0.585714% | 7.333 / 7.166 | 8.833 / 65.666 |
| 128 | 1 | 1 | 60.208 / 60.000 | 0.345469% | 72.125 / 68.292 | 185.375 / 205.834 |
| 128 | 1 | 2 | 60.416 / 59.750 | 1.102357% | 73.709 / 69.458 | 236.083 / 250.292 |
| 128 | 4 | 1 | 57.917 / 57.958 | -0.070791% | 66.709 / 66.917 | 195.209 / 182.125 |
| 128 | 4 | 2 | 57.792 / 57.541 | 0.434316% | 68.875 / 65.458 | 244.500 / 131.209 |
| 1024 | 1 | 1 | 626.375 / 639.417 | -2.082139% | 843.709 / 784.875 | 932.375 / 980.417 |
| 1024 | 1 | 2 | 630.083 / 632.625 | -0.403439% | 782.375 / 779.375 | 1341.458 / 930.083 |
| 1024 | 4 | 1 | 508.208 / 503.625 | 0.901796% | 634.708 / 603.042 | 832.083 / 866.667 |
| 1024 | 4 | 2 | 509.333 / 508.292 | 0.204385% | 641.375 / 667.333 | 767.458 / 744.125 |

Pooled control statistics use 384 observations per condition/cell. They remain a separate experiment and must not be pooled with the 48-sample formal runs:

| Panes | Views | P50 pair | Reduction | P95 pair | Maximum pair |
| --- | --- | ---: | ---: | ---: | ---: |
| 16 | 1 | 7.291 / 7.167 | 1.700727% | 9.083 / 9.083 | 30.459 / 40.709 |
| 16 | 4 | 7.000 / 7.042 | -0.600000% | 7.250 / 7.416 | 8.833 / 65.666 |
| 128 | 1 | 60.333 / 59.958 | 0.621550% | 72.625 / 68.292 | 236.083 / 250.292 |
| 128 | 4 | 57.875 / 57.750 | 0.215983% | 67.708 / 66.375 | 244.500 / 182.125 |
| 1024 | 1 | 628.125 / 636.250 | -1.293532% | 806.375 / 780.250 | 1341.458 / 980.417 |
| 1024 | 4 | 509.041 / 506.500 | 0.499174% | 636.459 / 616.833 | 832.083 / 866.667 |

### What this resolves, and what it leaves open

The formal 1,024-pane/four-view p50 slowdowns (0.185726%/0.626197%) turn into **0.901796%/0.204385% improvements** in the control. The formal 16-pane/four-view repeated negative direction also fails to repeat in both control groups: one is 1.785714% slower and the other 0.585714% faster. Thus the original negative direction is not a stable general result across these separately scheduled groups. Neither this reversal nor the added sample count proves that the original values were “noise.”

The one-view/1,024 control remains **2.082139% / 0.403439% slower**, absolute **13.042 / 2.542 us** per operation. Each width-stratified p50 also has the slower direction in both groups: group 1 is approximately 1.98–2.07% slower and group 2 approximately 0.10–0.72% slower. Equal alternating-width sample counts alone therefore do not explain that residual as a pooled-width artifact. This remains an observed limited slowdown, not an irreducible cost or a precisely attributed cause.

Those one-view p95s are faster in both control groups (**843.709 → 784.875 us**, **782.375 → 779.375 us**), while maxima are mixed (**932.375 → 980.417 us**, **1,341.458 → 930.083 us**). Four-view/1,024 p95/max directions are also mixed. Formal 128-pane/four-view maximum increases of 203/225 us do not repeat here: the control maxima decrease **195.209 → 182.125 us** and **244.500 → 131.209 us**. Conversely, tiny 16-pane/four-view control maxima increase from **8.041 → 39.958 us** and **8.833 → 65.666 us**. No unconditional tail/stability guarantee follows, and the control must not be cited as all-tail improvement.

There is no prespecified statistical equivalence band, randomized multi-machine analysis or causal ablation for resize. The supported stability statement is bounded: **the strong Move improvement survives the declared formal criteria; resize shows no stable broadly large median slowdown in this matrix, but modest one-view p50 shifts and variable tails remain unresolved and disclosed.** If “no regression” is intended literally as no negative estimate anywhere, these data do not support that phrase. Do not quietly introduce a new percentage tolerance to declare equivalence.

Source receipts independently show the benchmark driver, `layout.rs`, layout topology and layout reference source unchanged from the baseline. The actual Resize request writes bounds and does not call WorkspaceTransaction, canonicalization or MRU validation inside its measured endpoint. Fixture building/validation outside timing does execute changed helper paths. Executable code layout/inlining, allocator conditioning, caches, power state and scheduling are possible influences, but none was isolated or measured as the cause. The exact live success-state/export equality and unchanged source do not prove timing identity for the full process. No precise saved/lost microsecond budget is assigned to these mechanisms.

### Updated results-document closure

Re-read `docs/performance/2026-11-06-checked-move.md` at SHA-256 `2c04b348844da120b517f7522adb086bcf0ac055d353434e96ef7db6b63f3252`. It now includes the previously requested repeated 128-pane/four-view formal resize maxima, the separate follow-up settings and exact 1,024-pane before/after p50/p95 values, the 128-pane/four-view control maxima, retained formal negative evidence, unchanged timed layout/export source and explicit unresolved one-view limitation. All those added numbers agree with the independent arithmetic above. Its causal wording correctly lists possible influences without claiming they were proven, and it does not call the control a replacement for formal data or select further samples to erase residual negatives.

**Document disclosure recommendation closed. No new wording or evidence blocker is identified.** Root's remaining Clippy/doc/integration validation is still separately owned and is not rerun or inferred here. All previous main improvement tables and negative formal cells remain valid under their original source/executable identities.

| Supplemental artifact | SHA-256 |
| --- | --- |
| `resize-followup/environment.json` | `794b5c2743f91389cec3f5c522e25cab9b87c92af34c56b2ee77daf248889d72` |
| `resize-followup/raw.csv` | `8780772b28d39c1c609e96ecfa80e44eeecee98da394de744dc6d0ee9ed8a5be` |
| `resize-followup/summary.csv` | `7fa37e86fac4baa97dc1d30bceb62562ca0a930fcf7bbb2e9ec867962e3765d9` |
| `compare_resize_followup.py` | `637d34f4753f5eb067a10bf7cf22beb247aac51c815798cbdd9bdf5b500a9cc7` |
