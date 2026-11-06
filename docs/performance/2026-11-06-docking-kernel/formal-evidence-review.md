# Formal docking-kernel evidence review

Reviewed at 2026-11-06T13:45:29.643432+00:00. Evidence directory: `/tmp/dockspace-docking-kernel-20261106-a9ace58/formal`. Source label: `a9ace585e1e8d812efd35c6149f549043bb7c999`; recorded content hashes identify the actual modified working tree.

**Verdict: the formal data are internally consistent and support the declared limited renderer-free docking comparison. No blocking evidence finding was found.** All recorded source, executable and result hashes match; all raw samples, full dump sets, phase arithmetic and nearest-rank summaries pass independent read-only verification. This is not a runtime optimization result or a proof of general docking feature parity. The results-document and archive follow-up below also verifies the published arithmetic, declared scope and statistical wording.

This reviewer only read files and performed CSV/integer/percentile/hash arithmetic. No Cargo, compiler, benchmark, measurement binary, profiler, network or Git write was executed. Existing root-run data were not replaced or resampled. The only write is this report. Counter/nextest execution remains the root's separate serial work; the follow-up below verifies its completed existing artifacts without rerunning it.

## Completeness and actual observable state

- **8,064 raw rows:** 8 processes x 21 pane/view/case cells x 48 timed samples. Each process contains exactly 1,008 unique expected sample identities. Each original process CSV equals its exact part of the aggregate raw file. C++ stdout also equals its process CSV bytes.
- The 21 cells are three pane counts times seven valid view/case combinations: resize/center/edge at 1 and 4 views, cross-view only at 4. Total panes, not panes per view, is held fixed. All cases preserve `items = 2*initial_panes`; edge adds one leaf, one split and two nodes. All counts/root roster/width/height and resize `(warmup+sample)%2` parity match the agreed protocol in every raw row.
- All four phase values are nonnegative, every directly measured total is positive, and **request + mutation + layout + export equals operation_owned exactly for every sample**. A zero subphase value, visible in tiny resize requests, reflects timing resolution/optimization of minimal work; it does not establish a general zero-cost operation.
- All 27 distinct pane/view/case/width states have one shared digest across both implementations, groups, runs and samples. There are 21 cells plus the six second resize widths; both actual resize sizes were exercised.
- Every process has its exact **21-file normalized dump set**, 168 files total. Every corresponding full vector is byte-identical across all eight processes. This independently exercises the repaired per-process completeness gate rather than relying only on its success flag.
- I parsed the 21 unique full vectors: decimal u64 words, nine-word node prefixes, binary preorder paths, root/view ordinals, split axes, finite nonnegative integer geometry, recursive partitions with separator 4, correct total word/node/leaf/split/item counts, unique complete item ownership, selected membership and actual source/target intent all agree. Each vector has `9*actual_nodes + 2*initial_panes` words. The resulting FNV1a64, recomputed over eight little-endian bytes per word, equals every corresponding sample-0 raw digest.
- Sample-0 full dumps contain width **16380** because warmup is 8. Width 16444 is covered by every process's validated odd raw samples and cross-product digest equality, plus the source's complete outside-timer state oracle; there is no separately archived full width-16444 vector. Preserve this precise distinction instead of saying all individual samples were archived as full vectors.

The earlier observer-validation and dump-completeness review findings are closed in the source that produced this data. Native C++ state mutation still uses upstream handlers; full member/identity/Counts validation is outside timing. Both sides create the shared owned vector inside export and retain/read it after fixture destruction. Rich Rust projections survive through total stop and are destroyed afterwards. Real transaction temporary/replaced-state disposal remains timed.

## Statistical definitions

Each **group/product/pane/view/case** has **96 actual timed samples**, from two processes with 48 samples each. Each product/cell pooled across both groups has **192 raw samples**. These are actual per-operation rows, so explicitly named pooled percentiles may be recomputed from raw values; they are not pooled process-summary percentiles like the older complete headless benchmark.

Percentiles use nearest rank: `sorted(values)[ceil(n*percent/100)-1]`. All **84** archived group summary rows recompute exactly: all five metrics' p50/p95/max, **1,260 numerical summary entries**, and sample count 96. No row was dropped, duplicated or substituted.

Every ratio below is **dockspace time / Dear ImGui time**, so >1 means dockspace took longer for this declared operation/output subset. It is a ratio of separately computed raw p50 values, not the median of paired sample ratios, an average of process medians, an average of group ratios or a confidence interval. All times in the following tables are **microseconds**.

## 1,024-pane operation plus fresh owned export

Separate formal group results; each time pair is dockspace / ImGui:

| Views | Case | Group 1 ABBA p50 pair; ratio | Group 2 BAAB p50 pair; ratio |
| --- | --- | ---: | ---: |
| 1 | `resize` | 576.292 / 66.625; 8.649786x | 572.791 / 67.625; 8.470107x |
| 1 | `center_move` | 3349.084 / 67.458; 49.646951x | 3444.125 / 69.292; 49.704511x |
| 1 | `edge_split` | 3578.916 / 107.084; 33.421576x | 3634.208 / 109.292; 33.252278x |
| 4 | `resize` | 464.666 / 64.584; 7.194754x | 472.458 / 66.500; 7.104632x |
| 4 | `center_move` | 2298.167 / 66.000; 34.820712x | 2304.292 / 68.084; 33.844839x |
| 4 | `edge_split` | 2328.541 / 102.208; 22.782375x | 2347.750 / 103.917; 22.592550x |
| 4 | `cross_view_move` | 2271.334 / 66.000; 34.414152x | 2306.625 / 67.250; 34.299257x |

Pooled across both groups, **192 raw samples per product/cell**:

| Views | Case | Dockspace p50 | ImGui p50 | P50 ratio | P95 dockspace / ImGui | Max dockspace / ImGui |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | `resize` | 574.416 | 67.209 | 8.546712x | 689.375 / 82.292 | 881.875 / 192.084 |
| 1 | `center_move` | 3411.958 | 68.166 | 50.053663x | 3675.375 / 79.542 | 4011.208 / 172.333 |
| 1 | `edge_split` | 3600.291 | 108.000 | 33.336028x | 3968.583 / 124.000 | 4367.708 / 596.583 |
| 4 | `resize` | 469.459 | 65.416 | 7.176516x | 629.041 / 76.584 | 909.500 / 198.459 |
| 4 | `center_move` | 2300.916 | 67.416 | 34.130117x | 2662.833 / 77.875 | 3012.916 / 125.333 |
| 4 | `edge_split` | 2342.083 | 103.000 | 22.738670x | 2636.417 / 119.833 | 2908.417 / 262.750 |
| 4 | `cross_view_move` | 2293.458 | 66.542 | 34.466322x | 2489.916 / 80.708 | 2842.833 / 134.334 |

For example, one-view center move is **3.411958 ms / 0.068166 ms = 50.053663x** pooled, while its two separate group ratios are **49.646951x / 49.704511x**. That difference is valid: a ratio of pooled quantiles need not equal or lie between the ratios of group quantiles. Label the pooled table explicitly and preserve the group values; do not call 50.05x a group average.

At 1,024 panes, both groups agree on the direction and approximate scale of the gap for all seven cells. Across group cells the total ratios range **7.104632x–49.704511x**; the two-view-count resize range is approximately 7.10x–8.65x, while command cases range approximately 22.59x–49.70x. These ranges describe different cells or group estimates, not uncertainty bounds.

## Native completion before common export

The native endpoint is the original sample's first three contiguous phases: request + mutation + layout. I summed those phase values **within each raw sample**, then computed its percentile. This exactly identifies the timestamp before export; it is not total p50 minus export p50 or a sum of three phase medians. Pooled 1,024-pane results:

| Views | Case | Dockspace native p50 | ImGui native p50 | P50 ratio |
| --- | --- | ---: | ---: | ---: |
| 1 | `resize` | 517.167 | 22.125 | 23.374780x |
| 1 | `center_move` | 3336.833 | 23.625 | 141.241608x |
| 1 | `edge_split` | 3527.333 | 63.500 | 55.548551x |
| 4 | `resize` | 442.041 | 21.000 | 21.049571x |
| 4 | `center_move` | 2270.959 | 22.666 | 100.192314x |
| 4 | `edge_split` | 2307.083 | 58.375 | 39.521764x |
| 4 | `cross_view_move` | 2264.333 | 22.458 | 100.825229x |

This endpoint retains the inherent native representation/contract differences: Rust runs current proof capture, checked atomic transaction publication/canonicalization and complete richer immutable layout projections; ImGui mutates native state and performs its actual class/popup filter, command handling, cleanup/flags/info and geometry update. Shared fresh output makes the external observable result equal; it does not make those internal contracts identical or establish a language-level ratio.

Common export materially changes the ratio. This is expected and observable, not a reason to discard the native endpoint or hide the output ownership requirement. The data establish both endpoints on the same executions. They do not establish that every remaining difference is allocation, immutable ownership, checking, language overhead or irreducible correctness work.

## Phase attribution that is safe to quote

Pooled 1,024-pane **separate phase p50s**, in microseconds:

| Views | Case | Product | Request | Mutation | Layout | Export |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| 1 | `resize` | `dockspace` | 0.000 | 0.000 | 517.125 | 58.333 |
| 1 | `resize` | `dear_imgui` | 0.083 | 15.500 | 6.542 | 44.959 |
| 1 | `center_move` | `dockspace` | 125.542 | 2659.416 | 539.541 | 70.458 |
| 1 | `center_move` | `dear_imgui` | 0.125 | 16.958 | 6.541 | 44.333 |
| 1 | `edge_split` | `dockspace` | 125.084 | 2848.916 | 544.500 | 71.666 |
| 1 | `edge_split` | `dear_imgui` | 0.125 | 56.791 | 6.583 | 44.584 |
| 4 | `resize` | `dockspace` | 0.041 | 0.041 | 442.000 | 28.125 |
| 4 | `resize` | `dear_imgui` | 0.125 | 14.333 | 6.500 | 44.167 |
| 4 | `center_move` | `dockspace` | 149.208 | 1671.125 | 444.542 | 28.917 |
| 4 | `center_move` | `dear_imgui` | 0.167 | 15.916 | 6.500 | 44.500 |
| 4 | `edge_split` | `dockspace` | 148.292 | 1708.084 | 446.500 | 28.917 |
| 4 | `edge_split` | `dear_imgui` | 0.167 | 51.542 | 6.500 | 44.250 |
| 4 | `cross_view_move` | `dockspace` | 143.958 | 1671.583 | 443.000 | 28.792 |
| 4 | `cross_view_move` | `dear_imgui` | 0.166 | 15.792 | 6.500 | 43.875 |

These columns must not be added into a new p50 total. The direct totals above come from the independently timed same-execution endpoint. Their small numerical differences from a phase-median sum are ordinary quantile behavior.

The measured Rust mutation phase contains actual checked transaction publication and dominates changed-state cases. A directly computed **median of each sample's mutation/total fraction** gives: 1-view center_move: 78.175%; 1-view edge_split: 79.298%; 4-view center_move: 72.655%; 4-view edge_split: 73.228%; 4-view cross_view_move: 73.141%. This describes the measured category; it is not a claim that any specific validation or allocation can be deleted. Detailed root-cause attribution still needs the separate enabled-counter/source/CPU evidence and any isolated follow-up. The formal timing executable deliberately has the structural-work ledger compiled out.

C++ mutation includes **all-root `DockNodeUpdateForRootNode` even for pure resize**. Its separate layout phase is only the all-root position/size routine. Thus “ImGui layout is 6.5us” refers only to that phase; its native normalized endpoint and full owned output are larger, as the tables show. Do not repeat the old isolated layout number as a complete docking result.

## Environment, source and binary provenance

I independently rediscovered the runner's exact source scope using read-only `git ls-files` and checked all **324 relevant files**, including the two probes, root module/counter change, comparison script, root manifests/lock/toolchain and the core package's source. Their set and every current hash equal `formal/environment.json`. This is this kernel's relevant input scope, not a claim to hash the entire workspace's unrelated product/native packages. All **10** pinned ImGui source/header hashes match the current reference files. All **49 protected user files** retain their starting hashes.

Compiler/profile receipts state Rust 1.95.0 / LLVM 22.1.2 on aarch64-apple-darwin and Apple clang 21.0.0 on macOS 27 arm64. The Rust artifact is a production-profile test executable, opt-level 3, no debug assertions/overflow checks, with `DOCKSPACE_BENCH_NO_WORK_COUNTERS=1` and one Cargo job. Root Cargo.toml's production profile inherits release with ThinLTO/one codegen unit. C++ is `-O3 -DNDEBUG -flto=thin -std=c++17`, using pinned ImGui `d88f238d1d363ef387382b2e29ad12cf96f826ac`.

The fast formal Rust build receipt reused the existing valid Cargo artifact and copied it into the formal directory; it is not evidence of a fresh recompilation. The frozen executable's content hash and compiled profile identify what was run. Subsequent normal-counter/nextest builds may replace target artifacts without replacing these frozen executables.

| Artifact | SHA-256 |
| --- | --- |
| Frozen dockspace executable | `793e8f2c166cbee6cee4885b01052d25415c9ccac06fee4189be3945de96f6d7` |
| Frozen ImGui executable | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |
| raw.csv | `530dd5b37d2e559017017557262d4d45d2b1c4eb29f7cc677b35e117c03d141c` |
| summary.csv | `c5c9cf6a3e80dbadd4d7050c0d18fe806132122cdb13cb0689943e66e9f9e17d` |
| environment.json at review | `21b74f8422548f5304cd6bf43adace734f9011a5735ad02f3bfbfaa8d2a64777` |

The reviewed probe/runner hashes are `0994c837556c82fa9e3b21b81055a514a402cf24117c4e25edea0e04ca66e2f5` (Rust), `4c74fbb088618dff1c71d4eaaa4094889c4516d4ecef3fd2342eeeb9c855f36c` (C++), and `6a099e1f5fdc48da67e730576ec59baff9433d007c01e0f183f96e3c6146ffa5` (runner). Both frozen binary hashes and both recorded CSV result hashes match independently read bytes.

## Serial order and practical limits

The two build receipts and eight measured-process receipts all succeed, have positive intervals and are nonoverlapping in their actual order. Rust build: 13:38:11.694738–13:38:11.761336 UTC; C++ build: 13:38:11.774384–13:38:16.048480. Formal timing: **13:38:16.056663–13:38:58.361915 UTC**, group 1 dockspace/ImGui/ImGui/dockspace, group 2 ImGui/dockspace/dockspace/ImGui. No compilation is interleaved with these formal processes by this runner. That proves runner serialization, not absence of arbitrary desktop scheduling/power-state effects or a randomized multi-machine confidence interval.

Each sample has a newly constructed fixture outside timing, checked before the operation and destroyed afterwards. Those setup paths can affect caches and thermal state and make full process duration very different from the measured operation. The comparison does not measure cold context/application initialization or steady product frames. Warmups and ABBA/BAAB mitigate some order effects but do not erase all state/noise differences.

The actual timed work contains no egui UI, ImGui frame/tab widgets/text, font measurement, renderer/tessellation, GPU, native window/vsync, full paint recorder, Presented receipts or host settlement. One or four views mean logical roots/surfaces in one context/workspace; they do not prove native OS multiviewport lifecycle performance. Selected output is actual stored command intent, including ImGui pending `NextSelectedTabId`, not final visible-tab/scroll state.

The matrix intentionally has zero minima, inactive maxima, no central region, complete active two-tab fixtures, appended inactive payload moves and perpendicular binary splits. General constrained layout, central-region policy, n-ary/same-axis behavior, arbitrary focus/MRU history, undock/redock, failure/rollback and stale output authority are not measured by these rows. No throughput, input latency, RSS/heap allocation, per-scene memory saving, general feature parity or theoretical performance floor follows. Separate correctness suites remain necessary for those contracts.

The formal result therefore supports a concrete statement: **a large docking-command/layout cost gap remains after removing rendering and giving both implementations the same complete fresh owned observable state in the declared common subset.** The phase data localize much of current Rust command-case time to checked publication and layout/materialization; they do not by themselves identify a removable root cause. Final prose should quote the named endpoints, keep group/pooled definitions explicit and link the independent counter/source evidence when making a stronger causal explanation.


## Follow-up: independent counters, ordinary core checks and CPU ancestry

Audited at 2026-11-06T13:49:10.845844+00:00 after the formal arithmetic review. This adds only read-only checks of completed artifacts; no build, test, benchmark, profiler or binary was executed by this reviewer.

`work-counts.csv` contains exactly all **21** expected cells with no duplicates. Every row matches the source-derived predictions in `transaction-source-attribution.md`: for a command at total panes T/views V, original whole-workspace nodes N=2T-V and original per-root nodes n=2(T/V)-1, fingerprint builds are V+6; fingerprint visits N+6n; item scans 4; item visits 8T; transaction candidates 1; cloned nodes N; cloned tab items 2T. All seven emitted counters are zero for external-bounds resize.

At 1,024 panes, the measured facts are:

| Views | Fingerprint builds | Fingerprint node visits | Item scans / item visits | Workspace candidates / cloned nodes / tab items |
| --- | ---: | ---: | ---: | ---: |
| 1 | 7 | 14,329 | 4 / 8,192 | 1 / 2,047 / 2,048 |
| 4 | 10 | 5,110 | 4 / 8,192 | 1 / 2,044 / 2,048 |

Center, perpendicular edge and supported cross-view movement agree on these specific counters. Their equality does not establish identical total work: parent searches, source emptiness collection, individual Node clones, canonical post-move graph growth and other native operations are outside these particular counts. The evidence supports repeated root-value proof construction and full-workspace transaction/conservation scope as real mechanisms. It does not quantify their individually removable milliseconds, prove clone-free atomicity or authorize deleting checks. The counter build includes its measurement ledger and its clock values are not formal latency data.

The existing `nextest-core.log` states **1,943 tests run, 1,943 passed, 3 skipped**. The enabled-counter log reports its single `docking_work` probe passed. These are inspected root execution logs, not independently rerun suites; pending integration checks are not included in this statement.

The separate `profile-center` receipt names the frozen formal Rust executable; its current digest still matches `793e8f2c166cbee6cee4885b01052d25415c9ccac06fee4189be3945de96f6d7`. Probe and sampler return codes are 0, the capture requests eight seconds, and its CSV has **3,000** instrumented observations exclusively for 1,024 panes/one view/center_move. The capture launch timestamp, 2026-11-06 21:43:44 +0800, is after the formal timers ended at 13:38:58 UTC. Its 19.37-second whole-process wall duration is not a timer endpoint or the eight-second sampling duration.

This profile has **two** 5,997-sample thread roots: the waiting libtest main thread and the `docking_cost::docking_cost` worker. The blocked main thread must not be combined with worker samples to form a CPU-stage denominator. More importantly, worker samples include fresh fixture construction, pre/post validators and teardown outside the formal timers.

I parsed explicit call ancestry rather than assigning flat symbols to phases. In disjoint root unions within each named category, the graph contains 2,509 samples below visible `prepare_transaction_inner` roots; within that ancestry 1,096 are below visible canonical roots, 382 below strict `validate_workspace` roots and 65 below visible `Node::clone` roots. These **overlap** and must not be added. They are sampled observations, not operation/phase percentages or counts of clone calls. Inlining can hide more work. A further 1,080 samples are under visible canonical roots **outside explicit prepare ancestry**, demonstrating why a flat canonical/validation share cannot be equated with timed mutation.

The following actual ancestry is present:

```text
prepare_transaction_inner
  canonicalize_validated_draft
    validate_workspace
      Validator::walk
        graph::Node::clone
          allocation/copy descendants
```

There is also an actual `prepare_transaction_inner -> Workspace::clone -> Vec::clone -> Node::clone` ancestry. The frozen source independently shows `prepare_transaction_inner` calling checked canonicalization, `rewrite_subtree` cloning the node on Enter/Exit, and strict `Validator::walk` cloning visited node values. Thus individual node/vector copies and canonical/strict-validation work are confirmed inside the real transaction, not merely in fixture setup or external assertions. This supports the mechanism cited by the source trace while preserving the profile's unclean timing scope.

Supplemental artifact hashes at review:

- work-counts.csv: `d32325320b7b5281c82843ca4602bf34b9b117b42824c21f921f02ab0bb26dd8`
- transaction-source-attribution.md: `cbdc6c7de3d10e4d752e6fabe57b2e21e84847c67d239632b97fc41c6472c2dc`
- profile-center/center.sample.txt: `6ddbe81cba79359377817868148054bd318dbb0961e4d07414ea3d011fd8aa08`
- profile-center/receipt.json: `55bca81ed49f7eadfb87d367ada140e4f2e2783c2b771112713b76c3c69915e7`

No new blocker was found. The strongest supported causal statement is bounded: source plus counters identify repeated value-proof/global-inventory and copying work; sampled ancestry confirms that canonicalization/validation and Node copying actually occur inside checked transaction preparation. A flat sampled percentage or a claim that one pass accounts for the entire measured gap would exceed this evidence.


## Follow-up: results-document arithmetic, wording and archive

Reviewed at 2026-11-06T13:56:00.204386+00:00. The current results document is `docs/performance/2026-11-06-docking-kernel-results.md`, SHA-256 `76695eeab8caf82a99e1761c7055cb8f8a152aa710339453a1824688bdbe3cb5`; archive README SHA-256 is `bdaf2f014398992f3dd41ca8c974a0491ef3ff04edad669fa6a41ba607e9940c`. All checks here read existing CSV/text/gzip bytes; no probe, build, test or profiler was run.

**The results document's numerical tables and phase-share definition are correct. No new timing or statistical blocker was found.** I independently compared all 21 pooled operation-p50/ratio rows, all seven 1,024-pane group rows and all eight four-view phase-p50 rows directly with the original formal raw CSV. Every shown microsecond value and rounded ratio matches. Both reported p95/max examples match the pooled raw tails. The opening millisecond examples are correctly rounded.

The document's opening shares use `sum(phase_ns) / sum(operation_owned_ns)` across the same 192 pooled raw samples per product/cell. These differ from the median of sample fractions separately reported earlier in this review; neither statistic should be substituted for the other. Independent additive-share results for Rust at 1,024 panes are:

| Views | Case | Request % | Mutation % | Layout % | Export % |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | `resize` | 0.003197 | 0.002980 | 89.634370 | 10.359453 |
| 1 | `center_move` | 3.815743 | 78.161987 | 15.926672 | 2.095598 |
| 1 | `edge_split` | 3.569673 | 79.211615 | 15.178978 | 2.039734 |
| 4 | `resize` | 0.006641 | 0.006435 | 93.641990 | 6.344934 |
| 4 | `center_move` | 6.516932 | 72.552074 | 19.560070 | 1.370924 |
| 4 | `edge_split` | 6.481985 | 73.096711 | 19.130704 | 1.290600 |
| 4 | `cross_view_move` | 6.400091 | 72.719459 | 19.531038 | 1.349413 |

These reproduce the opening four-view move ranges 72.55–73.10% mutation, 19.13–19.56% layout, 6.40–6.52% request and 1.29–1.37% export, the one-view 78.16–79.21% mutation range, and resize layout 93.64%/89.63%. They are descriptions of observed phases, not isolated removable costs.

The archive's 42-row `formal/pooled-attribution.csv` is also independently recomputed: 630 p50/p95/max integer entries, 420 sum/share entries and all sample counts agree exactly or within floating-point serialization precision. Each cell contains 192 observations. Phase sums add to the total sum. This adds verification of the final pooled artifact to the already checked 84 group summaries.

The prose distinguishes logical roots from OS multiviewport lifecycle; pending selection intent from rendered TabBar state; common output equality from equal failure/rollback contracts; and source/counter/ancestry mechanisms from measured removable phase shares. It does not claim a Rust/C++ language floor, a full click or a post-refactor speedup. The proposed priorities remain recommendations grounded in repeated work, not an unmeasured performance guarantee. The ImGui source trace's one/two global reference scans and dense-ID `2N+3` generation queries are supported by the actual pinned handlers; they are properly labeled source facts rather than timing shares.

One nonblocking wording precision remains in the body at this review snapshot: the archive README correctly says that the 168 full vectors are **sample-zero dumps**, while the results evidence paragraph says “168 full-vector dumps” without explicitly repeating sample zero. Suggested wording: “Each process archives its sample-zero complete vector for every matrix cell; every measured sample is checked by the outside-timer complete-state oracle and cross-product digest.” This preserves the exact evidence boundary. In particular, the complete second resize width is not independently archived as a full vector. The opening “complete output vectors match” is supported by the archived corresponding vectors, but should be read with that explicit boundary.

Archive comparison at this packaging snapshot found 211 files with an original temporary counterpart; all original bytes match after decompression except `profile-center/receipt.json`, which deliberately preserves every original field/value and adds `settings` and `binary_sha256`. Those additions agree with the observed 3,000-row exclusive 1,024/one-view/center capture and the frozen binary hash; this is additive archive metadata, not a changed execution receipt. There are 72 gzip files in the snapshot. All six actual Markdown local links in the results document and archive README resolve.

`checks.json`, the `checks/` logs, `root-cause-review.md` and `SHA256SUMS` named in the README were still being assembled at this snapshot. Final archive manifest completeness and checksum sealing remain the root's packaging work; this is not a failure of the frozen formal data. The temporary evidence report has now been updated, so the archived copy and manifest should use this final version. No source or runtime change is needed for this review.
