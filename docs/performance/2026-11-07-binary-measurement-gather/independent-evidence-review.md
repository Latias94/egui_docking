# Binary measurement gather: retained evidence review

No material evidence or source-binding blocker was found. Immutable `49baba1b2b7ad8e152e8b4da180a2e130d5fce9f` supports retaining this bounded two-site measurement-gather change against actual control `37e388fbc95705204a42e4f8788623873c6ea711`. Independent reconstruction confirms 35 complete p50 improvements, five regressions and two ties; all 14 large cells improve. The ImGui threshold result is 30/42, with 12 small conditions still missing. P95 and max regressions remain material and must accompany the retention.

This reviews `docs/performance/2026-11-07-binary-measurement-gather.md`, its immutable evidence archive and the corresponding frozen files under `/tmp/dockspace-imgui-twofold-20261107-ea4161e/`. I independently parsed records, recomputed statistics and complete-vector checks, reconstructed source patches in memory and hashed frozen executables without executing them. No Cargo, compilation, tests, formatting, timing, runner, archived script, profiling, network or agent was run. No repository, fork, cache or candidate file was written; only this report was written. Root owns the recorded execution.

## Exact source, parent composition and formatting

The original author's source parent is `eb94cb242722962c9e0d88074d81cf462d784357`; the actual measured parent is the later retained exact-total `37e388f`. The application head is `9d669a425eb0f40f6e7483061168a1d0273ba98f`, after the rejected persistent experiment was restored. The archive keeps separate original source pairs, actual base, unformatted actual tree, formatted tree and measured source. Those stages agree and are not confused with one another.

| Object | SHA-256 |
| --- | --- |
| Original author patch, 4,732 bytes | `8db3c0cf34440a48be5563844a872e7e8d36fdf75bacd0d7c908d4b3ea19b7d2` |
| Actual frozen patch, 4,575 bytes | `bf40484c0df9eed1fbdb6d0ea02680b31af6f3d0d3ec819c81a9fdda31bd543b` |
| Formatting adjustment | `3ef38b5102e73cb0dc069b779bf066592e7d9602cc9f95018dd80008bd6579aa` |
| Application receipt | `5032135e75f0dce6878c0d1e46a5a37bb02d7072a3a3809fe4558c7d13f5138b` |
| Original manifest | `64cb72f102e579415acdad6fc6d6a027bd422e9c7eca53df7968efacc237fb2d` |
| Original READY | `5a1df199a894a4bcb1a9464e6d45ff4f113c3ef821d7ae919b32ea9da42961ff` |
| Original and durable source review | `2b0159aa447e354f43b70065ee07b32a4fbf9dce1131bb70b865aa36fc2c91d2` |

Both original complete base/tree pairs, Git blob identities, byte counts, source-set digests, four unchanged context files, artifacts and READY hashes match. Original base-set digest is `35bd51a4e08743cdb00c3c8afb606f5790190f0c984bc26fcb763ee9f98276d0`; original tree-set digest is `33725522820ea6e2e078f279a1a5b50dc4e07095ccbbee40ef9feb5b7cf74a4e`.

The original two-hunk patch exactly replays on the actual retained base and reconstructs the archived unformatted actual tree. That actual layout differs from the original author tree only by retained exact-assigned-total reuse. Each original hunk has an exact unique old fragment; this is not a whole-file copy from the earlier parent.

| Path under `crates/dockspace/src/` | Actual control SHA-256 | Measured/formatted SHA-256 |
| --- | --- | --- |
| `layout.rs` | `5da769be02038b5347555e4ed54c0ae72013c27e6a51c5dca97993804ce10bd7` | `ed2d1aaa966e89f94ebcc5d78f5deb4ad6e3b27a5d4d52a16d89af6da419dbf8` |
| `layout/topology.rs` | `1dd27eb635c84e0d73bf9186eb2e741f400b10e4b36bb6365477ac01c5bd21c6` | `1c0bd28862711316cbe9f963e46133a7f6440e774fc6686e5a1c056f626c94cb` |

The intermediate unformatted hashes are respectively `caf5cef3973bdfee0176b1f3b8c322c5fc6553a1e3d5311b7917c329b107c1f3` and `ab10ff27f57c3f03a2336daec731999af7d9c6038076cf4820d4ddf6a461fa44`. The formatting delta changes only the two fallback closures from an explicit block returning `SubtreeMeasurement` to the equivalent direct struct-literal expression. Their iteration order, record fields and borrow remain identical.

The first ordinary fmt receipt has return code 1 and a log containing exactly those two shape differences. Its 323 source hashes match the unformatted actual tree plus retained context. The later normal receipt differs only at these two files and records all 12 gates passing. The initial formatting failure was preserved rather than silently replaced or described as a semantic failure. Its receipt hash is `a8e14c29877be46a9d150122aa1adf667f0ab971f765fc337cac5f7bf685fdbc`; log hash is `7956b5bec08654aa6d98e37f46d414776cd0905b66953342fad8a172e5fc342e`.

The final frozen patch is exactly the immutable `git diff 37e388f 49baba1 -- crates/dockspace`: two files, two hunks, +50/-14 under that diff alignment. Its forward reconstruction equals the complete formatted/measured source; its inverse reconstructs the actual base. The original patch's +55/-15 count describes the pre-format source and different diff alignment, not the final measured representation.

All 324 measured source-receipt inputs match immutable `49baba1`; all 324 control receipt inputs match immutable `37e388f`. Only the two gather files differ. All 323 final ordinary-check inputs match the candidate receipt; the extra formal input is the unchanged C++ comparator. Candidate source receipt hash is `05616f277085a63cf14fbbc1a217942ff029628417a66445b3581f0d3d580de5`; ordinary-check receipt hash is `9c07edf3b79635d631ec48d628ecc282b37fe64dc3a24e1ddcad1b6298139f3f`.

The actual retained exact-total block is byte-identical in base, unformatted tree and final source. The entire numeric executor is also byte-identical to actual control, with function hash `468dd3e003bb01d56bbe273a7c07662650f8474d6e8b6444d604589c8a29b002`. Public solve_axis, raw reference, override tests, topology tests and the complete timed `docking_cost.rs` keep their control bytes. No rejected axis validation split/hints, SmallVec, fused continuation, persistent store, dependency or changed work metric is reintroduced.

## Mechanism and source limits

At exactly two child rows, each site materializes two complete `SubtreeMeasurement` records in a local array and passes an ordered borrowed slice to its original composer. The ordinary measurement site still calls `compose_constraints`; the existing delayed-maximum site still calls `compose_maximum` only when that pass runs. Every other arity retains the same reusable Vec, clear and ordered extend path.

No arithmetic evaluator, input validator, central-route authority, raw/reference compiler behavior, min/max/actual/gap outcome or fallible event order is replaced. The auxiliary `contains_central: false` fields were already false at these sites; the change does not infer central authority from them. Both records remain fully materialized, and fresh leaf answers, all three rich output buffers, complete owned export and old-output lifetimes remain.

A fully binary pass avoids populating that temporary child-values heap owner. The previous Vec was reusable, so this is not one allocation removed per split. Nary/mixed paths keep their allocations and copying; branch and local footprint costs remain. Neither deleted source work nor owner count proves an allocator timing fraction or a unique generated-code cause.

## Ordinary checks and unchanged work

All 12 final gate receipts have return code zero and serial intervals. Every full numbered nextest PASS inventory agrees with its summary; no anchored FAIL/TIMEOUT/ABORT/CANCELLED result was found.

| Gate | Passed | Skipped |
| --- | ---: | ---: |
| targeted | 432 | 1,644 |
| fixes | 4 | 2,072 |
| core-production, all features | 2,073 | 3 |
| public integrations, no default features | 4 | 0 |
| workspace, all features/all targets | 2,249 | 3 |
| host default | 292 | 0 |
| host serde | 335 | 0 |

The remaining gates are fmt, clippy, strict docs, doctests and the core dependency boundary. Final fmt has an empty successful log. Clippy retains its recorded correctness/suspicious policy; docs use `RUSTDOCFLAGS=-D warnings`. Doctests record 9 core, 0 conformance and 1 egui passes with zero failures. The normal/build all-features/all-targets core dependency boundary passes. Inventories overlap and are not summed into a unique total. The source-bound suites retain independent numeric/full-layout oracles and nary/raw/central/delayed/deep/staged/MRU/old-output coverage; this treatment adds or weakens no test.

The separate ordinary work probe records one pass and 2,075 skipped tests. Its full 21-row, 20-column CSV is byte-identical to the actual exact-total control and retained packed work CSV:

`88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4`

All deterministic counts were independently checked: zero structural/copy work for resize; original candidate/fork and logical inventory counts for commands; two payload clones; original tab/MRU and split-element copies; six completed edge bodies and 2,926 root rows. The original `node_handle_clones` column remains; the rejected persistent experiment's owner metric is absent. These are logical counters, not allocator/capacity measurements.

## Executable and comparison protocol

| Frozen executable | SHA-256 |
| --- | --- |
| Candidate, 18,750,720 bytes | `6d255458eac25cd9dcf629b88cfca6959c78db7ed2bb7ccf08fe2cba6332dcfd` |
| Actual control, 18,734,208 bytes | `ccbc1be2fe9d0c14b63e8fbed845fda6332148786485c539b9166d6efdcca5f3` |
| ImGui, 503,280 bytes | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

Independent hash-only reads match every environment/build receipt. Both Rust build outputs identify one selected dockspace executable and a successful build-finished record. They use optimization 3, debug information 0, debug assertions false and overflow checks false. Normal checks/work remove the counter bypass; formal build sets `DOCKSPACE_BENCH_NO_WORK_COUNTERS=1`. Both builds keep the existing unused `presentation_identity_frontier` warning; no warning-free result is claimed.

Both comparison campaigns use serial ABBA then BAAB, 48 samples/eight warmups per process/cell, and 96 observations per condition/group/cell. All 16 processes succeed, receipt intervals do not overlap, and stderr captures are empty. Rust stdout records the selected probe passing; all four ImGui stdout captures equal their process CSVs.

Every process contains the 21 protocol cells and 1,008 rows: 16/128/1024 panes, one/four views, resize/center/edge, and cross-view moves only at four views. Each campaign has 8,064 raw rows, giving 16,128 total. Aggregate raw rows exactly reproduce their individual process files. Every sample/product/topology/item/root/resize-width field matches the protocol, and complete-operation time equals the four recorded phase times' sum.

All 2,520 phase/stat summary fields were recomputed with nearest-rank p50/p95/max indices 47/91/95. All 1,008 numerical comparison fields across the durable 84 rows agree with those summaries, exact ratios and reductions.

All 336 complete vector captures were parsed for complete unique preorder topology, expected node/leaf/split counts, root and child geometry, four-unit split gaps, selected-item membership and every item exactly once. Little-endian-u64 FNV digests match their corresponding sample-zero observations. The 21 cell vectors agree byte for byte across products, conditions, groups/runs and campaigns. Every raw digest agrees at each of the 27 cell/actual-width keys, including both resize widths. No state failure or excluded cell is needed; this does not imply that a separate full vector was archived for every raw row.

## Recomputed results, all negatives retained

Positive reduction means faster performance.

| Reference | Statistic | Improve / regress / tie | Reduction range |
| --- | --- | --- | --- |
| actual `37e388f` control | p50 | 35 / 5 / 2 | -4.128592% to +16.666667% |
| actual `37e388f` control | p95 | 26 / 16 / 0 | -42.465098% to +67.941764% |
| actual `37e388f` control | max | 20 / 22 / 0 | -936.675928% to +97.354780% |
| ImGui | p50 | 3 / 39 / 0 | -246.560000% to +15.170605% |
| ImGui | p95 | 8 / 34 / 0 | -434.837800% to +18.520672% |
| ImGui | max | 14 / 28 / 0 | -1770.250522% to +78.947368% |

| Panes | Actual-control p50 improve / regress / tie | Actual-control reduction range | Candidate / ImGui p50 | At most 2x |
| --- | --- | --- | --- | ---: |
| 16 | 12 / 2 / 0 | -3.830180% to +16.666667% | 1.765537–3.465600x | 2/14 |
| 128 | 9 / 3 / 2 | -4.128592% to +3.906250% | 1.264414–1.592856x | 14/14 |
| 1024 | 14 / 0 / 0 | +1.228162% to +5.592651% | 0.848294–1.134263x | 14/14 |

All five actual-control median regressions in the document match the raw-derived complete table:

| Group / panes / views / case | Control → candidate ns | Reduction |
| --- | --- | --- |
| 1 / 16 / 4 / cross_view_move | 2,292 → 2,333 | -1.788831% |
| 1 / 128 / 1 / resize | 9,083 → 9,458 | -4.128592% |
| 1 / 128 / 1 / center_move | 10,583 → 10,750 | -1.578002% |
| 1 / 128 / 4 / cross_view_move | 11,000 → 11,208 | -1.890909% |
| 2 / 16 / 1 / center_move | 2,167 → 2,250 | -3.830180% |

The two ties are group 2 medium/one-view center at 10,625 ns and medium/four-view cross-view move at 11,125 ns. They were not counted as improvements.

The worst actual-control p95 is group 2 large/one-view edge, 137,958 to 196,542 ns. The worst max is group 1 small/one-view center, 3,959 to 41,042 ns. Large favorable tail changes remain too: group 2 large/one-view resize p95 is 235,250 to 75,417 ns and max is 3,199,167 to 84,625 ns. Such extreme favorable maxima coexist with 22 adverse max cells and do not justify a uniform-tail claim. Against ImGui, the worst max is group 2 small/four-view center, 958 to 17,917 ns.

The two small threshold successes are both one-view resize: group 1 candidate 1,250 / ImGui 625 ns = exactly 2.0x; group 2 candidate 1,250 / ImGui 708 ns = 1.765537x. The threshold is inclusive. The other 12 small conditions miss, while all medium and large conditions meet it, producing 30/42.

The historical retained exact-total comparison independently gives 28/42 from its immutable complete comparison table, hash `13f318fffad2c29e72b3967366f22042e064712b8641b4620b3d8407673c4720`. The new 30/42 result uses this candidate's own paired ImGui campaign; no ratios were synthesized by mixing phase timings or campaigns. Target improvement is bounded, not completion of the all-cell goal.

The complete-operation observations support the recorded retention while preserving every adverse cell. They do not isolate compiler placement, stack footprint, allocator share or a language-wide performance limit. Source simplification and threshold counts do not erase the five median and many tail regressions. The document's proposed subsequent bounded call-tree observation remains future work, not evidence executed by this review.

## Archive and final retention boundary

The 386,712-byte archive contains 452 unique safe regular members: 451 inputs plus manifest. Every path, size and SHA-256 matches. Each input was independently roundtripped to its frozen execution/original file or immutable source as appropriate: 427 execution/stage inputs, 18 original-package inputs, three actual-control inputs, two measured-candidate inputs and one combined source review. All six durable SHA256SUMS entries agree. Both complete original source pairs and all four unchanged context file bodies are actually present in the archive.

| Durable object | SHA-256 |
| --- | --- |
| Archive | `ca0f7fd1de90120de3a0cf7c0ecd9ccee624d802abf2112a19246d7c70207e29` |
| Archive manifest | `15c624e1af1302e036d24d791562ca323413508b3057c86c0ac5e4b2225ebf50` |
| Comparisons | `1ab639c66f188b90393b49510525976360652bf6687209d5c82d0f3b62250b49` |
| Decision | `e13c557dc1d76dc3e9aa8ba8667a43e11b601f73eda350af9b74a3eb445addcf` |

The decision's patch and binary hashes match the measured source and immutable adopted delta. Retention consists exactly of the formatted two-site change on actual `37e388f`, with exact-total reuse and all unchanged source inputs preserved. The original unqualified author READY and application-time `qualified: false` remain accurate preparation history; they are not substitutes for, or contradictions of, the later complete recorded gates and comparisons.
