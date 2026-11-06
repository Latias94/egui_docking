**Findings first**

No evidence-integrity blocker or unsupported retention claim was found in `docs/performance/2026-11-07-placement-sharing.md` and its evidence directory. The immutable source pair, all archived inputs, raw observations, complete output vectors, qualification receipts, work ledger, and all 84 published comparisons agree. The report explicitly retains the small-case regressions, tail regressions, whole-directory COW costs, and incomplete ImGui goal. Its decision to retain this bounded mechanism for observed large nonstructural mutation reduction is supported as a tradeoff; the evidence does not establish a uniform whole-operation win or completion of the twofold goal.

This is an independent read-only evidence audit. I did not run any archived runner, Cargo, compilation, test, benchmark, or timing command. Only `/tmp/placement-sharing-evidence-review.md` was written. No repository, candidate, fork, cache, network, or sub-agent work was performed. The passing/failing execution results below are claims checked against Root's archived receipts and logs, not checks rerun by this reviewer.

**Immutable identity and archive binding**

The candidate is `c98569d0b1424cb8ee0da724b1aba75b578bad78`; the actual control is `865409041a33cfb13442d5ea16f2035cf5feff0a`. I compared every one of the 324 source hashes in each frozen build receipt with its respective immutable commit. All matched. Both receipts have the same source-key set, and their only differing source is `crates/dockspace/src/graph/authority.rs`. Immutable diff inspection also finds only that core path changed; the comparison scripts and ImGui driver are unchanged. Current pointer-fix WIP was not used as candidate evidence.

| Exact evidence | SHA-256 |
| --- | --- |
| Report text, byte-identical to candidate commit | `26e340780de87d26e48b9509fc0ff502b37e10ac04a3d3b61ba88b8a2b4b596a` |
| Control `graph/authority.rs`, 13,734 bytes | `c4542993266ad497b50cdd30e00bbf6fc656819925d8d52942290b54d7d121e2` |
| Measured candidate `graph/authority.rs`, 18,482 bytes | `2c618031ea69e0dd3b4e6ccd16f7f34bbc4c52b1f733abaa515bf356ad4422d4` |
| Archived measured `runtime.patch` | `e73d8300df6c6832e0db13a0b5246043204549bdcbd498f10b9d69ca1313f3cb` |
| Candidate build/source receipt | `804f0e746d315e472e4eace3123fc3222afd8eb2adbd6b6803e56e672815b1ff` |
| Control MRU build/source receipt | `a6fb6f14ad893adb11a5d69c35d6b9d855feaf4eae3d6a163c109b81ba26f5d1` |
| `evidence.tar.xz`, 355,508 bytes | `94c5d8aff257e97fb576bf38ca447dc788fda4d8e17bce0b319705cb856c1ff3` |
| Archive's internal `manifest.json` | `3fe458540e634804b19d4a6ac88908adffb2350cee0238dace837e76e2f74af4` |
| Published `comparisons.csv` | `bae504d248a2161249e4dbc3f865268d30d393b093b893de583977147c2c3600` |

Archive member names below are relative to `evidence.tar.xz`. The candidate receipt is `placement-sharing-frozen/source-receipt.json`; control is `tabs-mru-frozen/source-receipt.json`. The actual source pair is preserved under `actual-control/` and `measured-candidate/`, with the complete control ID in `actual-control/commit.txt`.

There are exactly **436 regular input files plus one manifest**, 437 file members total. I checked each input's recorded byte length and SHA-256, the exact manifest/member set, and every external `SHA256SUMS` entry. All matched. Of those inputs, 433 also matched their retained original temporary/author files byte for byte; the remaining three generated source/commit records matched the immutable Git objects/identity. This independently supports the round-trip statement at report lines 82–85. No archive execution was involved.

The existing frozen executable files also match their recorded hashes:

| Condition | Frozen executable SHA-256 |
| --- | --- |
| Actual MRU control | `cba8437ec30e46186ec8c644b0474e8a97d4531ab34d0d727deab8f59728f31e` |
| Placement candidate | `edefa3e49f01b79a395b29125ca2b87acb467c50ce387e5b0856686dab88ae01` |
| Pinned ImGui pairing | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

The ImGui environment records the pinned reference `d88f238d1d363ef387382b2e29ad12cf96f826ac`. Both campaign environments bind the same candidate hash. Candidate build stdout identifies one dockspace executable with production opt-level 3, no debug assertions, and the recorded test profile. The freeze recipe records `DOCKSPACE_BENCH_NO_WORK_COUNTERS=1`; ordinary qualification explicitly removes that setting. The source itself is not replaced by a measurement-specific implementation.

**Qualification and the initial policy failure**

Report lines 16–34 accurately distinguish the initial failed fixture from final qualification.

- `placement-sharing-verified/initial-policy-fixture/targeted.log:305–306` records the new witness failing at the Native rehome command with `Policy(PresentationModeDisabled { mode: Native })`. Its summary at line 453 records 427 passed and one failed out of 428, with 1,644 excluded by the focused filter. The initial receipt preserves return codes 0 for fmt and 100 for targeted.
- The original candidate authority hash is `cae68a84471cc9fa64f74c7345c3566c8b7a3e8b84f870060b55ab2f4e44164a`. Reversing only archived `placement-sharing-test-policy.patch` from the final authority bytes reproduces that exact hash and the initial receipt. The entire production prefix before `#[cfg(test)]` is byte-identical. The correction changes policy imports and fixture setup, with no assertion removed or relaxed.
- The original patch `cfe9f0cb896c2db6aaa23de17d40328ae279a69038a6869698275fa84ec0e3c5` and policy-only patch `4279fc237f01773de9c61afee36c1e88bacc1e42dc28b42f51c1428bad45490a` remain archived, along with original review `e33a62cb137a9a54951ae378de25ff4e02e7dcaa5366940d347a244e85bd5122` and policy supplement `a0a734051e752cb4d8579fbc02b5f10822e5f8251f5c69b0389de159b8e49dc3`.

`placement-sharing-verified/checks-all.json` is explicitly merged from `checks-fmt,targeted.json` (two checks) and `checks-fixes,core-production,public-integrations,workspace,clippy,docs,doctests,dependencies,host-default,host-serde.json` (ten checks). I checked that their concatenated check entries equal all twelve final entries; both segments have identical 323-source maps, all return codes zero, normal counters enabled, and unchanged sources. Every one of those source hashes also matches the 324-input formal receipt. External `checks.json` reproduces those final entries. The merged receipt hash is `06e4b10f70d2e886bfeb923ab21f6cb79d0e49f28f3a61f9d5882f4a736c732c`.

| Archived log | Verified result |
| --- | --- |
| `targeted.log:435` | 428 passed; 1,644 filter exclusions |
| `fixes.log` summary | Four work/ownership regressions passed |
| `core-production.log:2076` | 2,069 passed; three opt-in probes skipped |
| `workspace.log:2254` | 2,245 passed; three opt-in probes skipped |
| `public-integrations.log:23` | Four no-default integration tests passed |
| `host-default.log:300` | 292 passed |
| `host-serde.log:343` | 335 passed |
| `clippy.log`, `docs.log`, `doctests.log`, `dependencies.log` | Successful completion consistent with their zero-return receipts |

The verification runner source establishes `clippy::correctness` and `clippy::suspicious` as denied, `RUSTDOCFLAGS=-D warnings` for docs, and ordinary counter settings. The wording does not claim broader all-warning Clippy enforcement. These qualifications bind the immutable placement candidate, not subsequent pointer work.

**Work inventory**

Report lines 36–40 match `placement-sharing-verified/work.csv`, its zero-return `work-check.json`, and the archived work log. I checked all 21 rows and all 20 columns. The CSV SHA-256 is `88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4`; it is also byte-identical to the retained actual MRU-control work CSV.

Resize rows have no transaction clone/copy/completion work at this owner. Center/cross rows have two actual tab payload copies and four MRU elements copied, with zero RootStructure completions. Edge rows have two MRU elements copied and one completion; its rows are `2 * (panes / views) + 1` and split handles are `panes / views`. All fingerprint and item-multiset rescan counts are zero. Arena/handle clone inventory remains present. Nothing in this ledger measures placement allocation count, bytes, or the Arc's empty/default cost, so the document's explicit limitation is necessary and correct.

**Full protocol, raw data, and vectors**

The two directories `placement-sharing-frozen/against-baseline/` and `.../against-imgui/` independently retain ABBA then BAAB process order. Each environment lists eight successful process runs, 48 measured samples and eight warmups per cell. Each process has the exact 21-cell set: 16/128/1024 panes; one/four views; resize/center/edge; cross-view only at four views. There are no missing or duplicate measured `(panes, views, case, sample)` keys.

I checked all 1,008 rows per process, recombined each campaign's raw CSV exactly from its process CSVs, and obtained 8,064 rows per campaign, **16,128 total**. Each condition/group/cell has exactly 96 observations. Every row has the expected complete node/leaf/split/item/root counts, alternating resize width, fixed other dimensions, nonnegative phases, positive complete duration, and exact sum of request/mutation/layout/export into `operation_owned_ns`.

All **336 complete vector files** were read and compared as full decimal-u64 word sequences. Every process/condition/reference yields identical bytes for each of the 21 cells. Each dumped vector's FNV digest matches its raw sample-zero observation. Across all raw rows, the digest agrees for all 27 `(panes, views, case, width)` combinations, including both resize widths. Dumps are the first measured sample per process/cell; this does not pretend that both resize widths have separate vector files.

The immutable probe independently checks the complete words for every observation, including after fixture destruction: `crates/dockspace/src/docking_cost.rs:794–801`. No solver, complete-output schema, leaf answer, or timer code changes in the source pair. Request capture, checked mutation, fresh leaf-constraint collection and all-root projection, fresh owned export, and the outer complete duration are visibly inside `measure` at `docking_cost.rs:441–463`. All-root leaf collection/projection remains at `:215–254`; export allocates a new words vector at `:329` and traverses every projection. The unchanged ImGui driver similarly includes request, dock processing plus root update, all-root position/size, and owned export at `integration/product-comparison/imgui_docking_cost.cpp:332–354`. This supports report lines 48–52; no truncated output or reused layout/export work is substituted.

**All summaries and comparisons**

I recomputed all five phases' p50, p95, and max from the raw observations, using the existing nearest-rank definition (`scripts/compare_products.py:104–105` at the immutable candidate). All 168 summary rows match, including the 96-observation count. Every numeric field in all **84 published comparison rows** then matches those raw-derived summaries, including both ratios and signed reductions. No p95 or max negative is omitted.

| Reference / statistic | Improvements | Regressions | Ties | Full reduction range |
| --- | ---: | ---: | ---: | --- |
| MRU control p50 | 17 | 24 | 1 | -7.7801% to +6.4650% |
| MRU control p95 | 19 | 21 | 2 | -23.7729% to +18.7959% |
| MRU control max | 17 | 25 | 0 | -757.0890% to +90.2302% |
| ImGui p50 | 0 | 42 | 0 | -380.0000% to -8.1628% |
| ImGui p95 | 0 | 42 | 0 | -475.2701% to -6.1431% |
| ImGui max | 6 | 36 | 0 | -2991.0749% to +63.6592% |

The control negatives described at report lines 61–66 are present in both groups where claimed: 16/one-view edge, 16/four-view cross, and 1024/one-view edge. The four 16-pane resize group/cells contain three regressions and one tie; all four 128-pane resize group/cells regress. The report's wording, “most small resize cells,” is accurate. Every such row is retained in the comparison table.

For concrete adverse witnesses, the worst control p50 is group 1, 128/one-view resize, 10,167→10,958 ns; worst p95 is group 1, 128/one-view edge, 22,084→27,334 ns; worst max is group 1, 1024/four-view center, 133,334→1,142,791 ns. That large max outlier is retained in the published CSV and range. No new significance or allocator-attribution claim is warranted from it.

**Phase reduction and retention judgment**

Report lines 54–59 are supported by the following exact summary rows against the actual MRU control:

| Group | 1024-pane cell | Mutation p50 control → candidate | Complete-operation p50 reduction |
| --- | --- | ---: | ---: |
| 1 | one-view center | 11,083 → 6,125 ns | 3.4214% |
| 1 | four-view center | 11,042 → 6,125 ns | 6.4277% |
| 1 | four-view cross | 11,042 → 6,166 ns | 2.1574% |
| 2 | one-view center | 11,083 → 6,125 ns | 3.7132% |
| 2 | four-view center | 11,125 → 6,125 ns | 5.5598% |
| 2 | four-view cross | 11,166 → 6,125 ns | 6.4650% |

Those are independently computed phase medians and complete medians; subtracting or summing them does not identify exclusive allocation time or reconstruct a complete-operation saving. The document says this explicitly. The source mechanism is consistent with the phase observation: `graph/authority.rs:88` shares the directory, `place_node` checks complete placement equality before writing at `:250–255`, and `forget_placement` probes the raw exact key at `:244–247`. Existing complete RootStructure installers retain full writes. Whole-directory COW still pays a full clone on its first shared true write. Empty owner allocation, handle operations, read probes, indirection, and the discardable follow-up's default construction are retained costs. This is a constrained explanation of observed timings, not proof of a uniform benefit or a new correctness capability.

| Panes | Recomputed candidate/ImGui p50 range | At most 2x |
| --- | ---: | ---: |
| 16 | 2.5328–4.8000x | 0/14 |
| 128 | 1.5231–2.1235x | 12/14 |
| 1024 | 1.0816–1.4447x | 14/14 |

The document's rounded table and 26/42 total at lines 70–77 are exact. Every p50 and p95 is slower than ImGui; 36 max cells are slower. “The goal remains active” is therefore the correct conclusion. Keeping the mechanism while explicitly acknowledging 24/42 complete-p50 control regressions is a stated engineering tradeoff. Nothing in this evidence supports relabeling it a hot-path winner across the full fixed matrix or marking the user's goal complete. The proposed next slice is clearly future work, not a claimed part of this candidate.

**Audit limits**

This review validates the stored single campaign, immutable source binding, complete-data arithmetic, and the wording of its decision. It is not a fresh benchmark or an independent execution replication. The evidence is sufficient for the report's bounded retention rationale; final overall performance and later native-pointer correctness remain separate Root-owned work.
