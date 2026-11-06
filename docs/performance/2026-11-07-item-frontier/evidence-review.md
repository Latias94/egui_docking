**Findings first**

No evidence-integrity or retention-judgment blocker was found in `docs/performance/2026-11-07-item-frontier.md` and its evidence directory. The immutable source pair, exact context rebase, archived profile branch, ordinary qualification, work inventory, complete raw observations/vectors, and all 84 comparisons agree. The document retains every reported regression and correctly leaves the twofold goal active. Keeping the narrowed Item completion for its removed duplicate writes and observed edge improvements is a supported bounded tradeoff; it is not a claim of uniform improvement or frontier-proportional total mutation.

This is a read-only evidence review. I executed no archived runner, Cargo, compilation, test, benchmark, or timing command. The only write was `/tmp/item-frontier-evidence-review.md`. No repository, candidate, fork, cache, network, or sub-agent work was performed. Execution outcomes below were checked against Root's stored receipts/logs, not rerun in this lane.

**Immutable source and exact rebase**

Candidate: `3dd308cc71a62184af31203b1119db11e5b504ed`. Actual control: `1c7169618e90834f1f9b007a59254c46aee8ed35`. The report bytes match the immutable candidate, SHA-256 `9b51aced68e6582481839fe48e25024ed9b2c7c1200ec600d166aeb7c1212b27`.

All 324 candidate source hashes in `item-frontier-frozen/source-receipt.json` match candidate Git objects; all 324 control hashes in `pointer-retirement-frozen/source-receipt.json` match the control objects. Their key sets are equal. Their exact source difference and the immutable core diff are the following three files. Neither current root-row-capacity work nor any later live source is used as candidate evidence.

| Source under `crates/dockspace/src/` | Exact control SHA-256 | Exact candidate SHA-256 |
| --- | --- | --- |
| `canonical.rs` | `b4d5abe7b5916e41efc8004f294aacf0623716a29e94d50a1fc9185a59fa03d4` | `dda19b95f3a8c29aca4e262b9b4239e1409ca8aceaf85ef2c3432c881aea79be` |
| `graph/authority.rs` | `2c618031ea69e0dd3b4e6ccd16f7f34bbc4c52b1f733abaa515bf356ad4422d4` | `89c65b0661ae3cc31b38b0157d4767e77c68219186ce78ca784ffb44d6af3fb9` |
| `graph/root_structure.rs` | `de30fe16e4587efe9cc181f5d1ca46b66250fa082c5b4363efa5f1c9a1215a62` | `a4746315ca204bad7759db02eaf9d95488b6169ec873dab6207766d669b8e710` |

The archive preserves these complete pairs under `actual-control/` and `measured-candidate/`. I reconstructed the rebased patch in memory from the archived control files and obtained the exact archived candidate bytes for all three.

The original patch is `fc27efcd998edaed8f1b3decf78c1aa38856f80d9bed87c817628be6fdc52c99`; the context rebase is `70cfba79fba97372f09fc4dcc8ae8ccc7d6e0ae396b922995aac125c4578b10d`. Both retain exactly **+38/-4**. Comparing addition/removal bodies by path gives exact equality. Comparing full hunk bodies finds one context adaptation: the existing full installer's `&mut self.authority.placements` becomes its current `Arc::make_mut(&mut self.authority.placements)`. No new implementation or test hunk is hidden in the rebase. The complete existing authority semantic test module is byte-identical, and all five prior COW sites remain.

The original review (`original-frontier/review.md`, hash `f7f67670517fc56d5fb3289fbee55ad784f0e2c923392ae0a495e74b1e0616fa`) and rebase review (`original-candidate/review.md`, hash `c0524918ee5af6bc7fda87f4a49708db5930e2b4aa9372703765b0046695da0e`) are both preserved. The latter equals the published review. This supports report lines 19–26 without treating either source review as executed performance evidence.

The source description also matches the delta. In candidate `canonical.rs:313–335`, retired raw keys are forgotten, surviving roots placed, and changed child joins refreshed before the only rows-only completion calls. `authority.rs:136–151` retains the ordinary full installer; the new `:154–167` entry completes only the concrete Item owner's Editing structure. Both entrances use the single `root_structure.rs:110` build loop; the ordinary wrapper at `:92–99` still installs placement and the Item wrapper at `:103–107` omits that repeated action. Rows, reads/errors, split handles and directory sorting remain. Birth/import/general/staged paths retain full installation. This evidence audit does not reopen the whole source review; it verifies that the measured implementation is the one described and reviewed.

**Archive identity and completeness**

`evidence.tar.xz` is 445,512 bytes, SHA-256 `f91cd6e795b7a0251fe920ccb43f90b83941b51d24c96de20f9074abb8af18af`. It contains exactly **448 regular inputs plus its manifest**, 449 file members. The internal manifest hash is `429b03875f6f02aa215c0bfb59e7c2dffa26cb2fcd66afe7042492746e45399c`.

I checked all manifest byte lengths/hashes, exact member membership, and every external `SHA256SUMS` entry. All agree. Of the inputs, 441 byte-match their retained original temporary/author/profile/qualification files; six generated source members match their immutable pairs and the generated commit record binds the control. The report's round-trip and three-pair statements at lines 72–76 are supported. Archived scripts were read as recipes only.

| Binding artifact inside archive | SHA-256 |
| --- | --- |
| Candidate source/build receipt | `582bc718b29d96381d68b7f371146051d1ef5428b396ced94a5483ed392280cd` |
| Actual control source/build receipt | `dd132fc97526474d7ac1e58f25d03c08f66c1765c92b61b6736c284106a84df3` |
| Candidate ordinary `checks-all.json` | `80bd5c9248cc375fa9b91d3d7875e71c31ce988f37453b95bd64b091023e4922` |
| Candidate `runtime.patch` | `45570e0874631cfd1d3ada26e3ed5199fcd00a8f86919da1f25787e2a6a93b8f` |
| Against-control `raw.csv` | `8d56e8756f2148308b33ccf7ff7f49c8830b91b918ed1add9229a6d4b111787f` |
| Against-ImGui `raw.csv` | `0ebed8dc9a731b0612fb48373b4b7569e3a4dafb3dfab914ead0dc6e34a27f4a` |
| Published `comparisons.csv` | `8e7746545f8b1f5ade3acb685abe51d8e5c20773c68a171e31483e7bcd4d7b06` |

Both campaign environments reference the same candidate receipt and frozen executable. Reading the existing binaries without running them reproduced their hashes:

| Condition | Frozen binary SHA-256 |
| --- | --- |
| Pointer-retirement control | `969d3223da4a540de3cdc1c5d26b6d10b788167c655998e35ef017929e75e2da` |
| Item completion candidate | `09a32eeefd9e3781133c6662cf917d97b5277a372e748a339425a6534313dfe9` |
| Pinned ImGui | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

The ImGui receipt records `d88f238d1d363ef387382b2e29ad12cf96f826ac`. The freeze recipe uses the existing production build with `DOCKSPACE_BENCH_NO_WORK_COUNTERS=1`; ordinary qualification removes that setting. Counters are disabled by configuration for formal timing, without replacing the candidate algorithm.

**Diagnostic profile attribution**

Report lines 14–17 are accurate as diagnostic sample attribution. The profile is from the earlier RootStructure diagnostic binary `d0facb66482a23377bcdaad5e3bb5be04de9fe6ee35f521a51d78e24cfdf9ba2`, not a final candidate/control measurement. The receipt explicitly says the sampled process includes fixture setup and attribution must be restricted to actual mutation/publish descendants.

`diagnostic-profile/sample.txt:2068–2070` gives the relevant ancestry: the running docking probe enters `prepare_transaction_inner`, then `complete_root_structure`, with **127 inclusive samples**. Its sibling RootStructure builder entries are 53, 31, 28, 4, 3, 2, 2, 2, 1, and 1, summing to 127. The directory branch is the 53-entry at `:2071`, comprising 48 and 5 `finish_directory` subentries (`:2072`, `:2095`). The placement branch is 31 at `:2096`, comprising 22 and 9 SparseSecondaryMap insertion subentries (`:2097`, `:2101`). The remaining sibling entries sum to **43**. The 53/31/43 categories do not double-count their nested frames.

Earlier same-named builder entries at `sample.txt:161–256` lie under `docking_cost::fixture → WorkspaceBuilder::build`, visible from `:48–49`; they are excluded. The profile's flat “total number in stack” table is also not used for this claim. The sample counts cannot be converted into exclusive nanoseconds, an allocator share, or a predicted saving. The preserved `diagnostic-raw.csv` has a separate 4,096 observations for 1024/one-view edge; it is not included in the campaign's 16,128 formal observations.

Profile text hash: `fc1df4a49e7b6c7b36c737afdfd55b5050f7ed8337dafb13424e7d3354d68053`. Profile receipt hash: `d9343a3f35a22fdb267b38c48b245b7fb8f01c07eb5ebab2615d545ee325ee02`. The document uses these counts to identify redundant work only, which is the supported interpretation.

**Ordinary qualification and work**

The candidate's twelve ordinary check entries all have zero return codes, run serially, record normal counters and unchanged sources, and bind a 323-source set contained exactly in the formal candidate receipt. Published `checks.json` equals those archived check entries. Relevant archived summaries are:

| Log under `item-frontier-verified/` | Verified stored result |
| --- | --- |
| `targeted.log` | 428 passed; 1,644 filter exclusions |
| `fixes.log` | Four work/ownership regressions passed |
| `core-production.log` | 2,069 passed; three opt-in probes skipped |
| `workspace.log` | 2,245 passed; three opt-in probes skipped |
| `public-integrations.log` | Four passed with no default features |
| `host-default.log`, `host-serde.log` | 292 and 335 passed |
| `fmt.log`, `clippy.log`, `docs.log`, `doctests.log`, `dependencies.log` | Success consistent with their zero-return receipts |

The recipe denies correctness/suspicious Clippy and sets `RUSTDOCFLAGS=-D warnings`; it does not claim all Clippy warnings were denied. These are exactly the gates stated at report lines 30–36. The opt-in work probe has its own successful receipt/log.

Actual control qualification is separately retained: `actual-control-verification/green-checks.json` has four successful checks; `integration-checks.json` has seven. I compared every recorded source hash from both receipts to immutable `1c71696` (three and 321 hashes respectively); all match. Native pointer retirement is therefore part of both source conditions, not the measured lever. The document correctly leaves remote physical CI as a separate requirement rather than treating these local receipts as that result.

`item-frontier-verified/work.csv` has exactly 21 cells and 20 columns and is byte-identical to `actual-control-work/work.csv`, hash `88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4`. Every row's inventory was checked. RootStructure completion remains zero for resize/center/cross and one per edge; each edge completes `2 * (panes / views) + 1` rows and `panes / views` split handles. Real MRU copies remain 0/4/2 for resize/center-or-cross/edge, with no fingerprint or item-multiset rescans. Unchanged counts establish that full rows/results still exist; these counters do not count placement insertion calls, allocation bytes, or prove exclusive runtime savings. The report preserves that distinction.

**Formal protocol, raw observations, and complete outputs**

The control and ImGui comparisons each contain independent ABBA then BAAB groups, eight successful process runs, 48 measured samples and eight warmups per process/cell. Each process has the full 21-cell matrix: 16/128/1024 panes; one/four views; resize, center, edge; cross-view only with four views. I checked the exact expected sample-key sets, with no missing/duplicate observations, and reconstructed each combined raw CSV byte-for-row from its process CSVs.

Each campaign has 8,064 observations, **16,128 total**, with exactly 96 per condition/group/cell. Every row has the complete expected nodes/leaves/splits/items/roots, original alternating resize width, dimensions, valid phase durations, and exact phase sum into `operation_owned_ns`.

All **336 complete vectors** were read. Full word-file bytes agree across products, conditions, runs, and groups for all 21 cells. Each dumped first measured sample's complete-u64 FNV digest matches its raw row. All raw digests agree over the 27 cell/width keys, including both resize widths. There is one vector dump per process/cell; every observation still receives the unchanged source-level complete-state validation.

The benchmark, layout/solver, comparison scripts and C++ driver are unchanged in the immutable source pair. `docking_cost.rs:441–463` retains request capture, checked mutation, fresh leaf/all-root layout, new owned export, and complete duration inside the original timer boundaries; its fixture destruction/full-vector checks remain outside timers in the same places. No scalar output, original cell, layout call, or owned export word is removed. This supports report lines 42–45 without implying the diagnostic sample is a formal timing run.

**Recomputed summaries and every negative**

All five phases' p50, p95, and max were recomputed from their complete 96-observation sets using the existing nearest-rank definition. All 168 campaign summary rows match. Every field in the published **84 comparison rows** then matches the raw-derived summaries, including baseline/candidate durations, ratios, and signed reductions. There is no tail-statistic or losing-group omission.

| Reference / statistic | Improvements | Regressions | Ties | Full reduction range |
| --- | ---: | ---: | ---: | --- |
| Actual control p50 | 24 | 17 | 1 | -7.8323% to +12.7525% |
| Actual control p95 | 18 | 24 | 0 | -20.9646% to +22.0788% |
| Actual control max | 25 | 17 | 0 | -381.6793% to +89.9254% |
| ImGui p50 | 0 | 42 | 0 | -373.4400% to -2.3349% |
| ImGui p95 | 0 | 42 | 0 | -394.5707% to -2.6595% |
| ImGui max | 7 | 35 | 0 | -754.0364% to +73.6485% |

Concrete negative witnesses remain present: group 2, 128/one-view center regresses p50 11,708→12,625 ns and p95 12,917→15,625 ns; group 2, 128/four-view resize max regresses 13,875→66,833 ns. The document's rounded ranges and warning against favorable-group selection at lines 47–58 are accurate.

The edge-specific claims are also exact:

| Cell | Group 1 complete p50 reduction | Group 2 complete p50 reduction |
| --- | ---: | ---: |
| 16 / one-view edge | +2.8039% | +5.5262% |
| 16 / four-view edge | -0.9839% | +0.0247% |
| 128 / one-view edge | +7.9223% | +8.3001% |
| 128 / four-view edge | +6.7164% | +1.2295% |
| 1024 / one-view edge | +12.7162% | +12.7525% |
| 1024 / four-view edge | +7.3702% | +3.0722% |

These are complete-operation medians; the profile's sample categories are not subtracted from them. The small four-view split result is retained as one loss and one near tie, not presented as a uniform edge win.

**ImGui goal and retention judgment**

| Panes | Recomputed complete p50 candidate/ImGui | Cells at most 2x |
| --- | ---: | ---: |
| 16 | 2.2952–4.7344x | 0/14 |
| 128 | 1.5256–1.9877x | 14/14 |
| 1024 | 1.0233–1.3439x | 14/14 |

The document's rounded table and **28/42** total at lines 60–68 match. All p50/p95 cells and 35 max cells still trail ImGui. All fourteen 16-pane cells remain over target, so the goal is incomplete despite every 128/1024 p50 cell meeting 2x in this campaign.

The retention language is proportionate: actual edge completion omits duplicate placement installation after concrete checked repair, while full row traversal, sorting, shared split acquisition, reference issuance, numeric repair, old-output ownership, and actual placement COW remain. A structural edit can still clone the shared placement table; the change does not make structural mutation O(frontier), nor establish an allocator share or significance from one median. The report preserves unrelated-path regressions and queues capacity/fresh-layout changes as separate future slices. No evidence or wording requires blocking this bounded retention decision.

This review validates the archived single campaign and its arithmetic/source provenance; it is not an independent remeasurement. Later capacity work and native physical CI remain separate Root-owned qualification.
