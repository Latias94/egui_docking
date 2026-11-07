# Exact assigned total: retained evidence review

## Conclusion and scope

No evidence or source-binding blocker was found for the bounded retention recorded in immutable commit `37e388fbc95705204a42e4f8788623873c6ea711`, against actual control `eb94cb242722962c9e0d88074d81cf462d784357`. The document, decision, complete comparisons and underlying observations agree. Retention is a mixed result: complete p50 has 26 improvements, 13 regressions and three ties; the large inventory has more regressions than improvements, and tail regressions remain substantial. The ImGui threshold remains 28/42.

This was an independent read-only evidence review of `docs/performance/2026-11-07-exact-assigned-total.md` and its archive at that commit. Immutable Git blobs, frozen receipts, complete archived records and hash-only reads of the three frozen executables were used. The live worktree was not used as measured source. No Cargo, compilation, tests, formatting, timing, runner, archived script, profile, network operation or subprocess agent was run. Only this report was written. Root owns the recorded execution and qualification.

## Exact treatment, source and executable binding

The actual runtime delta from the immutable control is one `layout.rs` hunk, +5/-1:

```rust
let assigned = if assigned == extent {
    assigned
} else {
    checked_sum(sizes.iter().copied(), "assigned size")?
};
```

It replaces only the original second checked assigned-size reduction. The whole-file inverse reconstructs the exact control. The first checked sum, strict grow/shrink comparisons and calls, all preceding numerical writes, and the entire suffix starting at `allocation_tolerance(...)` remain byte-identical. The exact equality branch cannot enter either intervening grow/shrink call. Repeating the same checked reduction over the unchanged slice would reproduce the first finite result; equality involving signed zero does not change that argument, because the retained scalar and the omitted reduction use the same unchanged inputs and reduction order. Every unequal case retains the original post-write checked sum and diagnostic. There is no tolerance shortcut or equality early return.

The single numerical executor, full input validation, minimum/maximum early outcomes, arithmetic order and final overflow/unallocated construction remain intact. No numeric inline hint, axis validation specialization, SmallVec, fused continuation, dependency or other queued treatment is mixed into the measured source. Existing reference/raw behavior, packed output buffers, fresh leaf inputs, timed probe functions and complete owned export retain their original bytes outside this one hunk.

| Bound object | SHA-256 |
| --- | --- |
| Original author patch, 568 bytes | `7cbd419b5d7d17a737fca1deb063b82ab812144767470b5e52d35d8c493da847` |
| Actual frozen runtime patch, 628 bytes | `0924aee8257aacd3719dd35c8e2410943379f5515c16d50302acc0743db0bc5b` |
| Control `layout.rs` | `ee0bbdadf60b10a3787539e89cdee41411301c3c774e66d53c0146ad1567eaec` |
| Retained/measured `layout.rs` | `5da769be02038b5347555e4ed54c0ae72013c27e6a51c5dca97993804ce10bd7` |
| Candidate executable | `ccbc1be2fe9d0c14b63e8fbed845fda6332148786485c539b9166d6efdcca5f3` |
| Packed control executable | `21888a3b2905da0edce33f68d4a7e3dbf68ba0e45d65f1d72df5d4d03f110435` |
| ImGui executable | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

Both patch representations reconstruct the same exact source pair; their formatting differs. The actual frozen patch is byte-identical to the immutable `git diff eb94cb2 37e388f -- crates/dockspace`. The original tree and measured candidate match the retained immutable blob; the original base and actual-control archive match the immutable control.

All 324 candidate receipt inputs were independently hashed against `37e388f`; all 324 control receipt inputs match `eb94cb2`. The sets differ only at `crates/dockspace/src/layout.rs`. All 323 ordinary-check inputs match the candidate receipt; the extra timing receipt input is the unchanged C++ comparator. The candidate receipt hash is `9e23a726ef4ed00135e7b7bb97614bd00c2f99622f2b26e7d77c7c2775af5fc1`; the normal-check receipt hash is `ced92e5516b16a1d671e95a4a373c2dc119acb1b6244332e3b746d5e7a783f8a`.

The recorded application head is `54deaf53b41fe6759aa667a2fb4a7bc22737d9f5`; its receipt explicitly names actual control `eb94cb2` and the original patch. Its historical `qualified: false`, and the author's source-only READY status, describe the preparation stage. The later execution receipts supply the qualification evidence; these stages were not conflated. The application receipt hash is `43ea9999b1229b0c868e199f854176c55c82946d148bf0db20815499e9fce79c`.

Frozen candidate and control executables were independently hashed, without execution; each is 18,734,208 bytes. Build output contains one selected dockspace executable and a successful build-finished record. Both use the recorded production test profile: optimization level 3, debug information 0, debug assertions false and overflow checks false. The formal build sets `DOCKSPACE_BENCH_NO_WORK_COUNTERS=1`; ordinary checks and work collection remove that bypass. This is the existing separation of normal correctness/work validation from formal timing. The recorded build has the existing unused `presentation_identity_frontier` warning; it is not described as warning-free.

## Ordinary gates and logical work

All 12 ordinary gates have return code zero, sequential receipt intervals and matching archived logs. For each nextest gate, the summary and every numbered PASS record agree; no anchored FAIL/TIMEOUT/ABORT/CANCELLED record was found.

| Gate | Recorded passing tests | Recorded skipped tests |
| --- | ---: | ---: |
| targeted | 432 | 1,644 |
| fixes | 4 | 2,072 |
| core-production, all features | 2,073 | 3 |
| public integrations, no default features | 4 | 0 |
| workspace, all features/all targets | 2,249 | 3 |
| host default | 292 | 0 |
| host serde | 335 | 0 |

The other five gates are fmt, clippy, strict docs, workspace doctests and the core dependency boundary. Fmt has an empty successful log. Clippy retains its recorded correctness/suspicious policy; docs use `RUSTDOCFLAGS=-D warnings`. Doctests record 9 core, 0 conformance and 1 egui successes with zero failures. The dependency log explicitly reports the normal/build all-features/all-targets boundary passed. These counts are overlapping gates, not a sum of distinct tests.

Recorded production PASS coverage includes the numerical/layout reference suite, the 10,000-nested-split cases, sparse/recycled identity and oracle results, previous outputs surviving late geometry failure, staged moves and rollback, ownership/MRU and existing central/raw contracts. No tests were added or weakened by this one-hunk treatment.

The separate ordinary work probe records one pass and 2,075 skipped tests. Its complete 21-row, 20-column CSV equals the packed control CSV byte for byte:

`88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4`

Every logical count was also recomputed from its row identity. Resize rows have zero recorded structural/copy work. Command rows retain one transaction candidate and one store fork, `2N-V` legacy cloned-node/node-handle counts, `2N` legacy cloned-tab-item counts and two payload clones. Edge tab/MRU copies are two; other command copies are four. Edge rows alone have two split-child and weight copies, one completed root structure, `2*(N/V)+1` root rows and `N/V` split handles. All six successful bodies and the aggregate 2,926 root rows match. These are logical work counters, with their existing compatibility names; they do not measure allocator calls, capacity bytes or phase latency.

## Complete comparison reconstruction

Both comparison campaigns were reconstructed from their individual process CSVs. Each records the same serial order: ABBA in group 1, then BAAB in group 2. All 16 processes succeed; receipt intervals do not overlap, including the transition between the two campaigns. All 16 stderr captures are empty. Rust stdout records the selected ignored probe passing; the four ImGui stdout captures equal their process CSVs.

There are 21 cells per process: 16/128/1024 panes, one/four views, resize/center move/edge split, with cross-view move only at four views. Each process has 48 retained samples per cell and the script records eight warmups. Each campaign therefore has 8,064 observations; together they have exactly 16,128. Each condition/group/cell summary uses 96 observations. The original complete-operation boundary remains request + checked mutation + fresh all-root layout + fresh owned export.

The aggregate raw CSVs exactly reproduce the ordered process rows. For every row, product, complete cell/sample coverage, topology counts, item/root counts and alternating resize width were checked. Every `operation_owned_ns` equals the sum of the four recorded phases. All 2,520 phase/stat summary fields were independently recomputed with nearest-rank p50 at sorted index 47, p95 at index 91 and max at index 95. All 1,008 numeric fields in the durable 84-row comparison table agree with those summaries and the ratio/reduction formulas.

All 336 complete vector files were parsed. Each has complete unique preorder topology, the expected leaf/split/node counts, root bounds, contained binary children and four-unit split gaps, and every item exactly once with valid selected membership. Their little-endian-u64 FNV digests match the corresponding sample-zero raw record. For each of the 21 cells, vectors are byte-identical across all products, conditions, runs and campaigns. All raw digest records also agree at each of the 27 cell/actual-width keys, including both resize widths. No state failure or cell exclusion was needed.

## Recomputed results and retained limitations

Positive reduction means the candidate is faster. Every negative and tie remains present in the full table and raw data.

| Reference | Statistic | Improve / regress / tie | Reduction range |
| --- | --- | --- | --- |
| actual packed control | p50 | 26 / 13 / 3 | -1.902174% to +8.320000% |
| actual packed control | p95 | 21 / 21 / 0 | -25.573333% to +20.827164% |
| actual packed control | max | 17 / 25 / 0 | -373.158448% to +88.100878% |
| ImGui | p50 | 2 / 40 / 0 | -262.218891% to +12.855522% |
| ImGui | p95 | 4 / 38 / 0 | -315.126050% to +18.039556% |
| ImGui | max | 8 / 34 / 0 | -773.981979% to +79.563660% |

| Panes | Actual-control p50 improve / regress / tie | Actual-control reduction range | Candidate / ImGui p50 | At most 2x ImGui |
| --- | --- | --- | --- | ---: |
| 16 | 9 / 2 / 3 | -1.902174% to +8.320000% | 2.064565–3.622189x | 0/14 |
| 128 | 11 / 3 / 0 | -1.265314% to +2.927473% | 1.248608–1.569694x | 14/14 |
| 1024 | 6 / 8 / 0 | -1.304719% to +1.793117% | 0.871445–1.164385x | 14/14 |

The worst actual-control median is group 1, 16 panes/one view/center move: 2,208 to 2,250 ns. Small-cell inventories differ between groups, 3/2/2 and 6/0/1. Large cells are narrowly mixed, not a general large-layout gain.

Tail examples remain material: group 1, small one-view edge split p95 rises from 3,750 to 4,709 ns; group 2, 128 panes/one-view edge split max rises from 22,834 to 108,041 ns. The largest favorable max change, 28,708 to 3,416 ns for group 2 small four-view center move, does not erase those regressions. Against ImGui, the worst max comparison is group 1, 128 panes/four-view cross-view move: 11,542 to 100,875 ns.

The only favorable ImGui p50 cells are the two large four-view edge groups. All 14 small cells still miss the 2x target, leaving 28/42 overall. The decision retains a bounded source simplification with mixed observations; it does not establish a uniform latency gain, allocator mechanism, guaranteed machine-code transformation or exclusive compiler cause. The source deletion alone cannot attribute the observed whole-operation changes to a measured phase share. Rich packed output, owned export, retained-output ownership and scene Arc costs are not removed by this scalar reuse.

## Archive integrity and historical boundaries

The immutable archive has 449 unique safe regular members: 448 inputs plus its manifest. All names, sizes and SHA-256 values were checked, and each input was independently compared to its frozen execution file, original candidate file, or immutable source blob as appropriate: 418 execution inputs, 26 original-candidate inputs, two actual-control inputs, one measured-candidate input and one combined source review. All six durable SHA256SUMS entries match.

| Object | SHA-256 |
| --- | --- |
| Archive, 387,384 bytes | `5a07b38b4a038cbbb761862ceff4d0bba0be97f9be2633e8826296f1f926f29f` |
| Archive manifest | `c49244dcadffac263f952e3480792c0814e6f4029e28abb552d04e81326a45ab` |
| Decision | `79e15411232c61e4855e7e6188016cd932dc34c2ce6e9c5c466ad5110c9116e5` |
| Original author manifest | `40dab27f1f7e96de0ab28bd83de695df6a1a49cd621f691a67ad7bcfe2c802ec` |
| Original READY | `f14a9ec06e98d17b729472556c7ebe6b19ee8bbd0f44aeb0c9d574c27be81840` |
| Original and durable source review | `1a7cfbfad2dda06e1bf038dcda7989960e016bfdaeaca2a61d1ba4eb02d7f84e` |

All nine original complete source pairs, Git blob IDs, byte counts, artifact hashes, READY bindings and sorted path-set digests agree with the exact immutable source. Base-set digest is `57fff24239fa8b86e922f63e419fc426fd5fdbb289bb6a144cc5fe47c3a11d01`; tree-set digest is `fbc0ee00b42669d1ebd940cc521eb29eaefbb245bce20f111d31493342053129`.

The archive includes eight Rust base/tree pairs, not the `Cargo.toml` file bodies: the packaging suffix filter excludes TOML. The ninth pair is fully hash/blob-bound in the archived original manifest and source receipts, and was checked against the unchanged original package and immutable Git blob. Thus the missing duplicate TOML body does not prevent reconstruction from the named immutable control. It is not described here as an archived TOML body.

The recorded decision and the immutable retained source agree exactly. Rejected numeric/fused/SmallVec changes and later live work are outside this retained runtime. No restoration is claimed for this retained hunk; the boundary is the precise source adopted in `37e388f`.

## Short appendix: queued patches on the actual retained parent

These are source replay checks only. Each candidate was considered separately on immutable `37e388f`, requiring every complete old hunk fragment to match exactly and uniquely in memory. This neither applies the patch to the repository nor supplies new qualification or performance evidence.

| Candidate | Original patch SHA-256 | Result on `37e388f` |
| --- | --- | --- |
| axis-inline-retained-packed | `beaf8e00c706878ec8924697db6d8dde5bdaefc6a9899f13dd3847c34f76f0ce` | All 16 hunks replay exactly; retained assigned-total hunk survives |
| binary-measurement-gather | `8db3c0cf34440a48be5563844a872e7e8d36fdf75bacd0d7c908d4b3ea19b7d2` | Both hunks, two files, replay exactly; retained assigned-total hunk survives |

For each of those two patches, applying it before or after the exact-assigned-total patch reconstructs identical composed bytes. Expected composed `layout.rs` hashes are respectively `c24e33bf531a697899647ac659cfec48957d1dd963135345bc6e1b981b5e6d88` and `caf5cef3973bdfee0176b1f3b8c322c5fc6553a1e3d5311b7917c329b107c1f3`. This does not claim that the two queued treatments were combined or mutually qualified.

Persistent-node-store patch `7d114b534dae5cb2a8b4ccbb43f9b9663a8e7ef402cbe0cea768694853cf8882` still has exact author-base bytes in all six modified files: workspace `Cargo.toml`, core `Cargo.toml`, `docking_cost.rs`, `drop_resolver.rs`, `graph/copy_tests.rs` and `graph/shared_storage.rs`. Each equals author parent `a2365aa876c51a66a5c9c788bff9056afdbabacd`; replay reconstructs the original six tree files. It does not touch `layout.rs`.

The five applicable baseline locks also remain byte-identical to that author parent and its dependency-boundary hashes: workspace, dockspace-host-harness, egui-fork-workspace, egui-official-harness and egui-product-harness. The package supplies no resolved candidate locks. Root must actually resolve all five applicable dependency graphs; this review does not guess their transitive versions or approve an unresolved lock set.

Subsequent adoption must replay each original patch on its actual retained parent, capture exact before/tree/lock receipts, qualify that composition and perform its own complete paired measurement against the appropriate frozen control. Copying an older whole layout file would erase the newly retained hunk. The static replay compatibility above supplies no benefit claim.
