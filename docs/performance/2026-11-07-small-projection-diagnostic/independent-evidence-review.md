# Current long-profile evidence review

No material blocker was found for the narrow location finding. The diagnostic source differs from retained runtime `49baba1b2b7ad8e152e8b4da180a2e130d5fce9f` only in the benchmark iteration-cap assertion. Both 500,000-row workloads succeed. Independent call-tree reconstruction confirms worker counts 2,905/2,941, strict non-fixture timed-layout inclusive samples 193/175, edge preparation 218, and projection-stack growth samples 25/18. Saved assembly matches the exact diagnostic binary and the sampled `project_measurements +4260` return address.

This supports investigating the existing projection continuation Vec's growth site. It does not establish allocator time share, operation-clock share, an allocation count, a latency lower bound, a score, or the benefit of a reserve change. The proposed reserve candidate has a separate source reviewer and requires its own qualification/measurement.

Only this report was written. I read the named long-profile scripts as data, the frozen records, the original short-profile records and immutable source. No Cargo, compilation, tests, runner, timing, sampling, disassembler, profiler, network or agent was executed. No repository, fork, cache, candidate or other report was written; no final-stop opinion was read.

## Diagnostic source and restoration binding

The formal source/build receipt is `binary-measurement-gather-frozen/source-receipt.json`, SHA-256 `05616f277085a63cf14fbbc1a217942ff029628417a66445b3581f0d3d580de5`. Every one of its 324 source hashes matches immutable `49baba1`. The long receipt's complete formal source map equals that formal receipt.

The saved original probe is byte-identical to immutable `crates/dockspace/src/docking_cost.rs`. Replacing this unique assertion reconstructs the whole diagnostic probe exactly:

```rust
assert!(value <= 1_000_000, "diagnostic iteration setting exceeds 1000000");
```

Its original limit/message is `10_000` / `benchmark iteration setting exceeds 10000`. The cap and its diagnostic message are the only source delta. All remaining 323 hashes are identical; the complete `measure`, fixture, input materialization, validation, export, destruction and loop behavior are otherwise unchanged.

| Object | SHA-256 |
| --- | --- |
| Original probe, 30,626 bytes | `01d8438fc79095aacae935170aff3ed7eb8b5e497aa368c9966889b789adf2d3` |
| Diagnostic probe, 30,632 bytes | `93879195c419a282c7bca446336af0a3b33d1e4854603cd71a6c8df5cb0afbf7` |
| Exact one-assertion patch | `c9f5a2f5f95c7b465a82714b272806e50ad7d0e483fe20ffebcd7f9c11d53c73` |
| Long-profile receipt | `862b0d4c8456815125eea5126b7896b7fd92584d849591ec0494275df94cda8b` |
| Producer script | `3c97fed2a18061a581203c164115c4c756f8d29768e2b27e7d55e4956609911f` |
| Classifier script | `80e89461a8649753dcf1246f6c2901d3fff3ba1442f889fb8a382cbad9682794` |

The producer's `finally` restores the saved original probe after checking that the temporary modified file was not concurrently replaced. It then compares all 324 formal source hashes before starting either workload. The receipt records restoration at `2026-11-07T05:16:57.746987+00:00`, preceding both run starts, and records both the before-run and final full-source comparisons as successful. The two complete source maps and saved probe bytes support that recorded restoration; this review does not claim to have observed the intermediate filesystem live.

The recorded build succeeds and selects one dockspace test executable. Its production artifact profile has optimization 3, debug information 0, debug assertions false and overflow checks false. The producer sets `DOCKSPACE_BENCH_NO_WORK_COUNTERS=1`. The source cap change does create a distinct executable; formal results remain attached to their original binary.

| Frozen executable, each 18,750,720 bytes | SHA-256 |
| --- | --- |
| Diagnostic | `b6c2b06483361b118d22c916ef0f06e1f9f8546d4d3d80eea11bf914a587e900` |
| Formal binary gather | `6d255458eac25cd9dcf629b88cfca6959c78db7ed2bb7ccf08fe2cba6332dcfd` |

Independent hash-only reads match both receipt values. No diagnostic measurement is substituted for a formal score, and no recompiled cap-extended binary is described as byte-identical to the formal executable.

## Workload completion and sample coverage

Both runs use 16 panes, eight warmups and 500,000 retained samples, one selected cell per process. Their sampler and workload return codes are zero; stdout records the exact ignored docking probe passing and stderr is empty. The selected cell/sample ordinal, topology/item/root counts, resize widths, phase-sum equality and stable digest per width were independently checked for every CSV row using streaming reads. No quantile or performance score was computed from these diagnostic rows.

| Diagnostic workload | Retained rows | Raw bytes | Actual sampled worker count | Non-fixture timed layout | Non-fixture preparation | Matched stack growth |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 16 panes / 4 views / resize | 500,000 | 45,299,850 | 2,905 | 193 | 0 | 25 |
| 16 panes / 1 view / edge split | 500,000 | 49,040,947 | 2,941 | 175 | 218 | 18 |

The `sample` commands request four seconds at a one-millisecond interval with `-mayDie`; both sampler logs explicitly report completed sampling of the recorded process IDs, 75955 and 76086. The process stdout durations are 9.85 and 10.43 seconds, so these runs persist through the requested observation. The actual worker counts come from the call graph, not an assumed 4,000 samples or from main-thread totals. The processes are serial.

Raw and `.csv.xz` hashes match their receipts. Streaming decompression reproduces exactly the raw byte length and SHA-256 for both files:

| Workload | Raw SHA-256 | Compressed SHA-256 |
| --- | --- | --- |
| resize | `92ea1e3a3f23586a67ab264ae857a3216d63669d88360ad620f66df293a67e50` | `f3e631fd4253a7bd6c0b9289393c8ceae822ca22a62d0511b78c90b872179db5` |
| edge split | `df06ceb43dfa9f2468cd79b4d7dcb8bebe83ba6867ee5255ae4e2021284a4502` | `c4fc7c0bc42d67c5e9f791ac43077992510daa2e8f3d83bafbaa734ac0af2fa4` |

The stable raw digests are resize width 16,380 → `0358a9379f081139`, width 16,444 → `1ac18ebff6f6893d`, and edge width 16,380 → `54302b714c824f3d`. This diagnostic intentionally removes the state-directory environment variable and does not capture full vectors. Full vector equality belongs to the separately reviewed formal comparison evidence.

## Strict ancestry reconstruction

I independently reconstructed parent/ancestor relationships from the sample call graph, rather than using its flat symbol totals. Selected roots must belong to the named docking worker, must have no `fixture` ancestor, and must not have an ancestor with the same selected phase token.

The eight disjoint outermost layout subtrees have inclusive counts:

- resize: 134, 16, 16, 9, 9, 6, 2, 1, totaling 193;
- edge: 112, 31, 12, 7, 5, 5, 2, 1, totaling 175.

Edge preparation has nine disjoint outermost `prepare_transaction_inner` subtrees: 108, 85, 8, 5, 3, 3, 3, 2, 1, totaling 218. Resize has none. These subtree counts are not logical root counts, operation counts or phase durations.

Every selected layout subtree has the same outer probe return site `docking_cost +2272`; every selected preparation subtree has `docking_cost +1828`. Thus the totals retain their distinct call-site ancestry as well as excluding named fixture frames.

Fixture work is explicitly excluded: `fixture` itself calls `project_all_roots`, and both sampled reports contain other instances of that same helper and the same projection return PC under fixture ancestry. A name or address match alone does not establish timed-operation provenance. In the exact probe, the selected non-fixture `project_all_roots` ancestry corresponds to the call inside `measure`.

The growth records are restricted to those selected layout subtrees and to exact specialization `grow_one::h370c7ae3627027d4`. Repeated matching ancestors are rejected before counting. The retained records are one 25-sample subtree for resize and two disjoint 16- and 2-sample subtrees for edge, totaling 18.

Every retained growth path is:

`docking_cost +2272 → project_all_roots +2164 → project_measurements +4260 → grow_one::h370c7ae3627027d4`

These growth samples are already included in 193/175. Parent and child counts, recursive frames, inlined aliases and repeated appearances of the same specialization cannot be added as independent work. The classifier's guard against repeated matching ancestry is appropriate; the retained growth entries themselves have no repeated same-specialization ancestor. Inline or unresolved direct probe work, post-return output/workspace drops and other ambiguous branches remain unclassified.

Attribution JSON hash is `d3c6ea9af24162e59d12a827c06422d1717f0379970fcf41670fc7f611d4c95f`; both its selected totals and complete retained growth paths match the independent reconstruction. Sample text hashes are resize `7a8aeab4b96d47f00a1b2ad302566d0e6cc64131405bbd2e71cc1df57befb4c0` and edge `d5dbe43cb0b4788181ac0ae5bb34af43eae246ea001550302ddfadb31c7bd536`.

## Exact binary/assembly/return-PC correlation

Without invoking a disassembler, I checked each saved instruction word against the frozen Mach-O text bytes: all 22 words in `project-stack-callsite.asm.txt` and all 26 words in `grow-one-48byte.asm.txt` match. Their named symbol addresses also match the binary's symbol table. Both sample images have the diagnostic binary's UUID `07CCE261-E294-361C-B624-513AF7ACA039`.

| Bound address | Value |
| --- | --- |
| Unslid `project_measurements` entry | `0x100b37808` |
| Conditional growth call | `0x100b388a8` |
| Return PC immediately after call | `0x100b388ac` |
| Return PC minus function entry | 4,260 bytes |
| Exact `grow_one` target | `0x100c4813c` |

The saved ARM64 `bl` instruction decodes to that exact target. After applying each sample's own ASLR slide, the sampled `+4260` addresses resolve to `0x100b388ac`: resize runtime address `0x1053008ac` with slide `0x47c8000`, and edge runtime address `0x10512c8ac` with slide `0x45f4000`.

The growth helper passes alignment 8 and element size 48 to `finish_grow`. The caller then computes an element address with a 48-byte stride and stores a zero variant tag, row and four bound scalars. This matches `ProjectionFrame::Enter { row, bounds }` at the existing second push in `advance_split_projection`, whose code is inlined into this `project_measurements` image. Immutable source has the ordinary `Vec<ProjectionFrame>`, initial root Enter and ContinueSplit/child Enter pushes. The 48-byte finding concerns the Vec element, not a sampled allocator-call count.

The exact assembly hashes are callsite `cdf148a831b4e5941bbbfdd618513cb0911060018ea302bddacc0de112cb85de` and growth helper `d3c81ecb9de8c607dfd5349138e2588c1ebce75c55ab57ac02d23edf5dec47d7`. This ties the saved assembly and source interpretation to the sampled diagnostic executable; it does not transfer this exact code layout to the different formal binary or a future candidate.

## Original short observation and conclusion limits

The earlier `current-small-profile` remains a separate preserved artifact. Its receipt still binds the unchanged formal binary and original 10,000-row cap. Both existing raw/compressed hashes and row counts match that receipt. Its resize sample has no named docking worker; its edge sample has only 107 worker observations. The recorded processes end before the requested four-second observation can cover a long run. Successful command returns do not repair that limited coverage.

Short receipt hash is `b0313961879844b606cbb8a7c35e141600d319873d63b75dabff52999491a90a`. Short sample hashes remain resize `e9931e33fd89f1f36737777c38623a63fe3c3b360c9c8c7283673f4ceee132fb` and edge `c9e52ae6583c23569b81d790de458a33e3787016cdac92ffd5384baa13e62fec`. This review did not replace those records with the long results.

The long observation establishes that continuation-stack growth occurs on an identifiable, non-fixture timed-layout path in both selected workloads. It supplies a concrete location for a separately reviewed reserve experiment. No sample count here is converted to nanoseconds, allocator share, complete-operation share, exclusive causality, attainable speedup or an architecture-wide floor. The unobserved/ambiguous work and all fixture, validation and post-clock destruction costs remain part of the whole probe's sampling environment. The proposed single-site `min(nodes, 8)` reserve is neither implemented nor approved by this evidence review.
