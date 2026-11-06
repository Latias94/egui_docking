# First checked-move iteration: independent evidence review

Reviewed at 2026-11-06T14:53:50.087884+00:00. Evidence root: `/tmp/dockspace-checked-move-20261106-a2bcabc`. Baseline label: `a2bcabc1440caa664fb5f4a64c4669c4eecfdce4`.

**Verdict: the first-iteration data are complete and internally consistent; no blocking measurement-integrity finding was identified. The plan/goal gate is not met.** At 1,024 total panes, four-view center p50 improves **19.144560% in group 1 and 18.789775% in group 2**, below the required 20% in each group. These failed cells must be retained. One-view center improves 34.356648%/35.799109%; that does not substitute for the four-view gate. The threshold is the plan's investment criterion, not a claim that the user supplied that number.

This review used existing text/CSV/binary bytes and independent integer, percentile, digest and hash arithmetic only. No Cargo, compiler, test, benchmark, measurement executable, profiler, network, Git write or child agent was run. The only write is this report. The source is already undergoing a recorded second bounded iteration; first-iteration receipts identify v1, not the current v2 tree. This report does not review v2 source or qualify its performance.

## Completeness, arithmetic and output agreement

Two distinct datasets were audited independently:

- `frozen-comparison`: first Rust candidate against the frozen Rust baseline, with baseline/candidate/candidate/baseline then candidate/baseline/baseline/candidate order.
- `candidate-reference`: the same first candidate executable against pinned actual Dear ImGui, with Rust/ImGui/ImGui/Rust then ImGui/Rust/Rust/ImGui order. This is reference context, not the baseline-improvement experiment.

Each dataset contains exactly **8,064 raw rows**, eight successful processes, 21 cells per process and 48 observations per cell. No sample is missing, duplicated or unexpectedly added. Every original process CSV equals its exact contribution to the aggregate; C++ stdout equals its corresponding process CSV bytes. Each group/product-or-condition/cell contains **96 observations**, and each cell pooled over both groups contains **192**. Cells use total panes 16/128/1,024 with one/four roots; cross-view exists only for four roots.

For all **16,128 raw rows**, request/mutation/layout/export are nonnegative, the direct total is positive, and the four phases sum **exactly** to `operation_owned_ns`. Node, leaf, split, item, root counts and bounds match the declared workload: edge adds two nodes/one leaf/one split, items remain twice initial panes, root count stays fixed, and resize alternates width 16,380/16,444 with height 16,380. Digest syntax is valid lower hexadecimal.

Each dataset has **84 group summary rows**. I independently recomputed all five metrics' nearest-rank p50/p95/max: **1,260 numerical entries per dataset, 2,520 in total**, plus every sample count. All agree. Nearest rank is `sorted(values)[ceil(n*p/100)-1]`; raw pooling is justified here because actual per-operation samples are retained. These are not process-summary percentile estimates.

Every process has its exact 21-file sample-zero dump set: **168 full vectors per dataset, 336 total**. Every corresponding vector is byte-identical across the baseline, candidate and ImGui processes in both datasets. I independently parsed all 21 unique vectors, checking the nine-word node schema, root ordinals, binary preorder/path numbering, split kinds, integer local geometry, separator-4 half partitions, complete unique ordered item membership, selected membership and expected moved-item placement/intent. Word counts are `9*actual_nodes + 2*initial_panes`. Recomputed FNV1a64 over each word's eight little-endian bytes matches each corresponding sample-zero raw digest.

Across both datasets, all **27** distinct cell/width states have one consistent digest: 21 primary cells plus six second resize widths. Full dump files archive sample zero only, whose resize width is 16,380 after eight warmups. The second width is covered by raw digests and the existing probes' complete outside-timer state oracle; no complete width-16,444 vector is separately dumped. Digest agreement is not a claim that every sample has a separately archived full vector.

## Baseline/candidate improvement at 1,024 panes

All times below are **microseconds**, each pair **baseline / candidate**. Improvement is `100 * (1 - candidate_p50 / baseline_p50)`. It is a ratio of each side's separately calculated p50, not the median of paired ratios or an average of group estimates.

| Views | Case | Group 1 p50 pair; improvement | Group 2 p50 pair; improvement |
| --- | --- | ---: | ---: |
| 1 | `resize` | 593.083 / 594.250; -0.196768% | 586.417 / 587.333; -0.156203% |
| 1 | `center_move` | 3471.791 / 2279.000; 34.356648% | 3476.458 / 2231.917; 35.799109% |
| 1 | `edge_split` | 3691.833 / 2431.541; 34.137297% | 3691.709 / 2423.291; 34.358559% |
| 4 | `resize` | 470.500 / 470.917; -0.088629% | 473.125 / 470.834; 0.484227% |
| 4 | `center_move` | 2357.500 / 1906.167; 19.144560% | 2357.000 / 1914.125; 18.789775% |
| 4 | `edge_split` | 2407.458 / 1964.958; 18.380383% | 2401.500 / 1962.042; 18.299313% |
| 4 | `cross_view_move` | 2340.416 / 1971.375; 15.768180% | 2356.583 / 1968.834; 16.453866% |

The four-view center shortfalls are approximately **0.855440 and 1.210225 percentage points**, respectively. Neither rounding 19.145 to 20 nor substituting the pooled result is acceptable. Edge and cross-view improve in both groups; this satisfies their direction condition but does not repair the center gate.

Pooled results use all 192 observations per condition/cell and preserve the tails:

| Views | Case | Pooled p50 pair | Improvement | P95 pair | Maximum pair |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | `resize` | 590.792 / 589.125 | 0.282164% | 694.333 / 702.542 | 851.125 / 1454.666 |
| 1 | `center_move` | 3473.042 / 2248.042 | 35.271672% | 3738.125 / 2610.833 | 4020.000 / 2846.334 |
| 1 | `edge_split` | 3691.833 / 2425.667 | 34.296405% | 4223.458 / 2672.584 | 4567.916 / 2944.125 |
| 4 | `resize` | 472.084 / 470.917 | 0.247202% | 616.625 / 613.708 | 1005.167 / 811.833 |
| 4 | `center_move` | 2357.500 / 1909.625 | 18.997879% | 2671.958 / 2113.000 | 3507.042 / 3869.416 |
| 4 | `edge_split` | 2407.375 / 1962.208 | 18.491801% | 2615.916 / 2180.292 | 2884.333 / 2384.709 |
| 4 | `cross_view_move` | 2349.750 / 1971.375 | 16.102777% | 2624.708 / 2231.542 | 2763.458 / 2381.375 |

One-view resize group p50 is 0.196768%/0.156203% slower, while four-view resize is 0.088629% slower/0.484227% faster. The pooled resize estimates happen to improve by about 0.25–0.28%; quantiles are nonlinear, so this is compatible with the group results. These tiny estimates should not be promoted into a resize speedup or silently rounded away as zero. They do not establish a material repeated regression either.

The tails are not uniformly improved. The candidate one-view resize maximum is **1,454.666 us versus 851.125 us**, and four-view center maximum is **3,869.416 us versus 3,507.042 us**. Its center p95 still improves. No such samples were removed, and no maximum is presented as a latency guarantee. ABBA/BAAB on one desktop does not provide a randomized multi-machine confidence interval or control every scheduler/power-state effect.

## Phase context and work counts

The following are independent pooled phase p50s from the frozen comparison; **do not add them to reconstruct the p50 total**:

| Views | Case | Condition | Request | Mutation | Layout | Export | Direct total |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | `center_move` | baseline | 131.166 | 2724.000 | 548.917 | 71.875 | 3473.042 |
| 1 | `center_move` | candidate | 133.916 | 1491.667 | 554.542 | 69.125 | 2248.042 |
| 4 | `center_move` | baseline | 153.916 | 1716.416 | 453.250 | 29.541 | 2357.500 |
| 4 | `center_move` | candidate | 156.958 | 1269.917 | 451.542 | 29.000 | 1909.625 |
| 4 | `edge_split` | baseline | 155.750 | 1757.875 | 455.916 | 29.167 | 2407.375 |
| 4 | `edge_split` | candidate | 156.958 | 1317.208 | 453.417 | 28.917 | 1962.208 |
| 4 | `cross_view_move` | baseline | 148.666 | 1715.584 | 450.541 | 29.292 | 2349.750 |
| 4 | `cross_view_move` | candidate | 151.750 | 1336.125 | 452.542 | 29.041 | 1971.375 |

The substantial movement is in the mutation category, consistent with this implementation slice. The timings do not isolate how much improvement belongs to root snapshot construction versus borrowing in canonicalization/validation, nor do they establish the removal value of the next iteration's work.

`work-counts.csv` has exactly all **21** expected cells. Every count matches the source-derived complete request-plus-command model. For total panes T, roots V, original nodes N=2T-V and per-root nodes n=2(T/V)-1, the unchanged request builds V roots. A same-root move builds one current command root; a cross-view move builds two. Hence builds are V+1 or V+2, and visits are N+n or N+2n. Resize has zero in all seven emitted counts.

| First-iteration 1,024-pane workload | Fingerprint builds | Node visits | Item scans / occurrences | Candidate clones / nodes / tab items |
| --- | ---: | ---: | ---: | ---: |
| One-view center/edge | 2 | 4,094 | 4 / 8,192 | 1 / 2,047 / 2,048 |
| Four-view center/edge | 5 | 2,555 | 4 / 8,192 | 1 / 2,044 / 2,048 |
| Four-view cross | 6 | 3,066 | 4 / 8,192 | 1 / 2,044 / 2,048 |

This ledger is the whole measured driver, not only the candidate's private check. Its request portion remains intact. The four item-multiset scans and one full candidate clone remain: no checking/conservation/rollback pass has been removed to obtain these measurements. This ordinary enabled-counter build is separate from the frozen timed executable; its clock values are not formal latency. The existing log reports the `docking_work` probe passed. No counter execution was rerun for this review.

## Candidate versus reference context

This separate execution sequence reports the first candidate against actual pinned ImGui. The ratio is **candidate / ImGui**, so values above one mean the candidate took longer for the declared endpoint. It must not be mixed with the earlier baseline/candidate raw pool.

| Views | Case | Candidate pooled p50 | ImGui pooled p50 | Ratio |
| --- | --- | ---: | ---: | ---: |
| 1 | `resize` | 594.167 | 72.458 | 8.200157x |
| 1 | `center_move` | 2265.250 | 73.625 | 30.767402x |
| 1 | `edge_split` | 2460.958 | 115.500 | 21.306996x |
| 4 | `resize` | 476.708 | 70.042 | 6.806031x |
| 4 | `center_move` | 1933.875 | 71.625 | 27.000000x |
| 4 | `edge_split` | 1981.500 | 110.416 | 17.945769x |
| 4 | `cross_view_move` | 1993.875 | 72.708 | 27.423048x |

These remain renderer-free request + native command/normalization + all-root layout + the same fresh owned complete export. Fixture construction, outside oracle, digest, IO and result destruction are outside timing. Real Rust transaction temporaries/replaced workspace destruction remain inside apply. Logical roots are not OS viewport lifecycle; selected command intent is not rendered TabBar visibility, scroll or a complete click. Rust's atomic failure/stale-proof/MRU contract and richer immutable intermediate layout outputs remain distinct from ImGui's native live-tree mutation. No language-level floor, allocation-byte saving, RSS change, feature parity or theoretical irreducible gap follows.

## Frozen v1 provenance and actual sequence

The frozen comparison's `candidate_sources` equals the candidate-reference receipt's **325-file** source map exactly. All six hashes in `reviewed-runtime.json` equal those recorded source values and occur in the earlier independent implementation review's snapshot table. Against the prior baseline receipt, the only differences are the five changed tracked files (operation/workspace/canonical/validation plus the resolver test expectation) and the new `checked_move.rs`. The unchanged Rust/C++ probes, common runner and compiler inputs retain their existing hashes.

All frozen executable hashes still match current bytes. The baseline executable is byte-identical to the previously audited formal baseline (`793e...6d7`); the candidate-reference Rust executable is the exact candidate used in frozen comparison (`5018...099`). The ImGui executable equals the earlier pinned executable (`32fa...6c0`). All 10 recorded pinned ImGui input hashes match both the prior receipt and current reference files. The candidate is the recorded production-profile test artifact, opt-level 3, debug assertions/overflow checks disabled, ThinLTO/one codegen unit, and the structural ledger compiled out via `DOCKSPACE_BENCH_NO_WORK_COUNTERS=1`. Ordinary counter/tests use their own ledger-enabled build.

Current source intentionally differs from v1 in `graph.rs`, `operation.rs`, `validation.rs` and `workspace.rs` as the second bounded iteration proceeds. That is a post-measurement iteration, not evidence that the v1 executable changed during its run. The v1 receipt's end-of-run unchanged-input check and its frozen binary/source hashes remain the relevant provenance. This reviewer did not relabel v2 as v1 or require current-tree equality. All **49 protected user files** independently retain their starting hashes.

I checked **18** recorded build/process intervals across the two datasets: positive, successful and serial without overlap in their actual order. Candidate Rust build ran 14:39:18.804407–14:42:26.381822 UTC, then C++ build completed at 14:42:31.141985. Candidate/reference measurements ran **14:42:31.150706–14:43:13.912037 UTC**. Frozen baseline/candidate measurements followed at **14:43:46.439052–14:44:02.772103 UTC**. No recorded build is interleaved with these measurement processes. This verifies runner serialization; it cannot establish the absence of arbitrary unrelated desktop activity.

| Artifact relative to evidence root | SHA-256 |
| --- | --- |
| `baseline-docking-cost` | `793e8f2c166cbee6cee4885b01052d25415c9ccac06fee4189be3945de96f6d7` |
| `candidate-reference/dockspace_docking_cost` | `5018da13ea92d8496348467db1fe1298941c5d3f59d14335a5bb0cf144842099` |
| `candidate-reference/imgui_docking_cost` | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |
| `frozen-comparison/environment.json` | `ba9d769a905568ba13ef64e6f522c34a1cceb14ab33fccb353bc31978a67851c` |
| `frozen-comparison/raw.csv` | `f3855d125dd8c30fbce393bf75a8af47886683b2482fe86382736055573dea21` |
| `frozen-comparison/summary.csv` | `83e269b6b882021ea18412a95a4ac5d3fa24d06d1220e05f96982820e2261ebf` |
| `candidate-reference/environment.json` | `bf12fe0f141e9fcd0371545818601d35623470ef66c697c487ef47f1c7be9fe8` |
| `candidate-reference/raw.csv` | `7d6e73560a1a83b33fed304872495969e56e5ea518d0e175fa26e0734485dd85` |
| `candidate-reference/summary.csv` | `c24137e7724228bdab6ea39caf3154796d90be3dc4f7b86a7ff0d34d797a6315` |
| `work-counts.csv` | `d096a75896956878bd6229ddf498ad059358c31337f235900940bde9c0e31b3e` |
| `reviewed-runtime.json` | `e526008a24022d4fd0a7f6c03eb2a8d21dffbe3afaf1042991741896e89808cc` |
| `compare_frozen.py` | `f0a072dae76ebf6295caa33f43a8f67bab2db8955857685e0107ee4f86796feb` |

The first iteration therefore supplies a valid failed-gate result and concrete attribution for a bounded follow-up. Preserve it under its own source/executable identity; do not replace its raw rows, pool it with a later candidate, claim the 20% center goal was achieved, or predict the next result from this review. A second candidate requires its own final source review, completed checks, frozen executable and both serial comparison groups.
