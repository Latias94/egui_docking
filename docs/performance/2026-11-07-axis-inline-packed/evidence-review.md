# Axis inline on packed results: rejection evidence review

No material evidence or binding blocker was found. Immutable `a53fac37f2c217c47fbc2c1988e5e509dc3faa6e` supports rejecting this exact candidate against actual control `37e388fbc95705204a42e4f8788623873c6ea711`: all 42 complete p50 cells regress, including all 14 large cells. Ordinary correctness, logical work and complete-output equality pass, but do not establish a performance benefit. The restored immutable sources preserve the retained exact-assigned-total change.

This is an independent review of `docs/performance/2026-11-07-axis-inline-packed.md` and its evidence directory at that immutable commit. I parsed archived data, independently recomputed statistics and vector checks, reconstructed patches in memory, and hashed frozen executables without running them. No Cargo, compilation, tests, formatting, timing, runner, archived script, profiling, network or agent was executed. No repository, fork, cache or candidate file was written; only this report was written. Root owns the recorded execution.

## Two source parents and the measured executable

The original author's parent is `eb94cb242722962c9e0d88074d81cf462d784357`. The measured parent is the later retained exact-total control `37e388fbc95705204a42e4f8788623873c6ea711`. The original package and its source review describe the first parent; the actual-base/tree snapshots, application receipt, ordinary-check receipt and frozen build describe the second. They are not interchangeable.

| Object | SHA-256 |
| --- | --- |
| Original author patch, 13,296 bytes | `beaf8e00c706878ec8924697db6d8dde5bdaefc6a9899f13dd3847c34f76f0ce` |
| Actual frozen patch, 13,780 bytes | `a68025c99c6740c3c06eb1987820fc470e686f32354f0046ce57a3380afea155` |
| Application receipt | `6f628e433f7bce415172794b0ddc1eaa285640ca45431e97b4d9bec29c951243` |
| Candidate source/build receipt | `bac7b73d0ac70afd085a7722952b409971c473d8b20798cad42fd443d7f0bd21` |
| Ordinary-check source receipt | `02a564346d04f63639bcfe414e9b0b8e8b7e4d3efb646608f6f30407d1d14634` |
| Candidate executable | `67b620bf388324979f016b495cbe078299d7dab313cb7a70ad81049c2bc66cfb` |
| Actual exact-total control executable | `ccbc1be2fe9d0c14b63e8fbed845fda6332148786485c539b9166d6efdcca5f3` |
| ImGui executable | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

All nine original source pairs, byte counts, artifact/input hashes, source-set digests and READY bindings agree with the original package and immutable author parent. Original manifest hash is `60bb5dc3cb47fd2bc55456023e5e420cb5b72990f650520cf7f665666371c69d`; READY hash is `cbff9218e9c63a5a1579353d0f9dbc6cdf506215e4c17eb4e2c648853c62e4e5`. The base-set digest is `57fff24239fa8b86e922f63e419fc426fd5fdbb289bb6a144cc5fe47c3a11d01`; original tree-set digest is `f387ab0ae35a0cd3730ca193f91650ea3ff466d016a1b3659fd0ea548e75b671`.

Replaying the original patch on the actual immutable control exactly reconstructs the archived actual tree. Conversely, adding the retained exact-total hunk to the original author's axis tree produces the same actual tree. The two representations have 16 identical complete old/new hunk fragments. Their header positions differ after the four-line exact-total addition, and the diff alignment inside the `validate_extent` extraction hunk differs; the actual source transformation is identical. The frozen patch is not incorrectly treated as byte-identical to the author patch.

| Actual changed path under `crates/dockspace/src/` | Actual base/restored SHA-256 | Measured tree SHA-256 |
| --- | --- | --- |
| `layout.rs` | `5da769be02038b5347555e4ed54c0ae72013c27e6a51c5dca97993804ce10bd7` | `c24e33bf531a697899647ac659cfec48957d1dd963135345bc6e1b981b5e6d88` |
| `layout/projection.rs` | `fb6fb6454034656114bfdcb80049f53fbbc325a780af00f720efd7769fa1494e` | `91c4d74339acf3c015f23c3d03a4942650af91a5b6f81a9a1f2dfc073660233c` |
| `layout/topology_tests.rs` | `67671850120413fbaec896b7eea930b7eb52086ccf4864b8486ab21ed8ea1ad2` | `5710f7ac168d7d5aa7af53ad4dd208e47668c9d658a9cfe856d020a9507955c5` |

All 324 candidate receipt inputs match the actual control plus exactly those three measured files. All 324 control receipt inputs match immutable `37e388f`; they also match the restored immutable `a53fac3`. All 323 ordinary-check inputs match the candidate receipt. The timing receipt adds only the unchanged C++ comparator input. Application-time `qualified: false` and the original source-only READY remain valid historical preparation records, distinct from subsequent execution.

The treatment is runtime +32/-29 across two files, plus the original 201 test lines in the third. It keeps one numeric executor, raw/selected-override full validation, committed nonoverride extent validation at the original position, and two ordinary hints on continuation advance and SplitMap construction. The public solver and the entire `solve_validated_axis_into` function are byte-identical to actual control, including exact-total reuse. The numerical executor's function hash is `468dd3e003bb01d56bbe273a7c07662650f8474d6e8b6444d604589c8a29b002`. No numeric-executor hint, SmallVec, fused continuation, persistent storage, dependency change or other experiment is included.

The source binding preserves packed stores/copies, fresh inputs, delayed child/gap phases, raw/reference behavior and the unchanged whole `docking_cost.rs`. Original rejection and source packages nested under `original-candidate/inputs/` are historical source context, not additional executed treatments.

Independent hash-only reads match all frozen binaries: candidate 18,768,512 bytes, actual control 18,734,208 bytes, ImGui 503,280 bytes. Candidate/control build records each select one dockspace executable and finish successfully with the same production test profile: optimization 3, debug information 0, debug assertions false, overflow checks false. Ordinary validation and work collection remove the counter bypass; formal build sets `DOCKSPACE_BENCH_NO_WORK_COUNTERS=1`. Both formal builds retain the existing unused `presentation_identity_frontier` warning; no warning-free result is asserted.

## Ordinary gates and logical work

All 12 ordinary gate receipts have return code zero and serial intervals. Each nextest log's full numbered PASS inventory agrees with its summary; no anchored FAIL/TIMEOUT/ABORT/CANCELLED result was found.

| Gate | Passed | Skipped |
| --- | ---: | ---: |
| targeted | 434 | 1,644 |
| fixes | 4 | 2,074 |
| core-production, all features | 2,075 | 3 |
| public integrations, no default features | 4 | 0 |
| workspace, all features/all targets | 2,251 | 3 |
| host default | 292 | 0 |
| host serde | 335 | 0 |

The other five gates are fmt, clippy, strict docs, doctests and the core dependency boundary. Fmt has an empty successful log; clippy uses its recorded correctness/suspicious policy. The read-only script inspection confirms docs use `RUSTDOCFLAGS=-D warnings`. Doctests record 9 core, 0 conformance and 1 egui passes with zero failures. The dependency log reports the all-features/all-targets normal/build boundary passed. Gate inventories overlap and are not summed into a unique test total.

The production log explicitly includes both original axis regressions: `committed_axis_inputs_keep_infinite_extent_and_delayed_error_priority` and `raw_axis_inputs_keep_earlier_geometry_before_later_bad_weights`. Original numeric/full-layout oracles, deep/raw/staged/old-output coverage remain present through the exact source binding and full recorded gates.

The separate work probe has one pass and 2,077 skipped tests. Its 21-row/20-column CSV is byte-identical to both actual exact-total control and the retained packed work CSV:

`88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4`

Every row's logical formula was independently checked. Resize records zero structural/copy work. Command rows retain one candidate/store fork, `2N-V` legacy cloned-node and handle counts, `2N` legacy cloned-tab-item counts, two payload clones, and the expected two/four tab/MRU copies. Edge alone records two split-child/weight copies, one completed root, `2*(N/V)+1` root rows and `N/V` split handles. The six completed bodies and 2,926 total root rows match. These counters do not establish allocator counts or explain the measured slowdown.

## Complete observations, outputs and statistics

Both campaigns retain the unchanged serial ABBA then BAAB order, 48 samples/eight warmups per process/cell and 96 observations per condition/group/cell. All 16 processes succeed, their recorded intervals do not overlap, and all stderr captures are empty. Rust stdout records the selected probe passing; the four ImGui stdout captures equal their process CSVs.

Each process has all 21 cells and 1,008 rows: panes 16/128/1024, views one/four, resize/center/edge, and cross-view moves only at four views. Each campaign has exactly 8,064 rows; total is 16,128. Aggregate raw rows exactly reproduce ordered individual process CSVs. Every row has the expected product, sample set, topology/item/root counts and resize width. Complete-operation time exactly equals the four phase times' sum.

All 2,520 phase/stat summary fields were independently recomputed at nearest-rank indices 47/91/95 for p50/p95/max. All 1,008 numerical fields in the complete 84-row comparisons agree with those summaries, ratios and reductions. The document and decision counts/ranges match these independent calculations.

All 336 complete vector captures were parsed and checked: complete unique preorder topology, node/leaf/split counts, root/child bounds and four-unit split gaps, selected-item membership and exact once-only item inventory. Little-endian-u64 FNV digests match the corresponding sample-zero records. All 21 cell vectors agree byte for byte across products, conditions, runs and campaigns; all raw digests agree at each of the 27 cell/actual-width keys, including both resize widths. This checks the complete archived vector captures and all raw digest records without claiming that a separate full vector was archived for each raw sample.

Positive reduction means faster; negative values remain fully included.

| Reference | Statistic | Improve / regress / tie | Reduction range |
| --- | --- | --- | --- |
| actual `37e388f` control | p50 | 0 / 42 / 0 | -25.543791% to -2.344404% |
| actual `37e388f` control | p95 | 4 / 38 / 0 | -92.116757% to +6.384065% |
| actual `37e388f` control | max | 12 / 30 / 0 | -706.306487% to +88.153810% |
| ImGui | p50 | 0 / 42 / 0 | -306.606607% to -5.851480% |
| ImGui | p95 | 0 / 42 / 0 | -907.201987% to -4.267881% |
| ImGui | max | 9 / 33 / 0 | -4381.918819% to +84.342892% |

| Panes | Actual-control p50 regression range, all 14 cells | Candidate / ImGui p50 | At most 2x ImGui |
| --- | --- | --- | ---: |
| 16 | 2.344404%–20.536344% | 2.532800–4.066066x | 0/14 |
| 128 | 12.180563%–24.108004% | 1.492172–1.854731x | 14/14 |
| 1024 | 15.051049%–25.543791% | 1.058515–1.422318x | 14/14 |

The worst actual-control median is group 2 large/four-view resize, 76,500 to 96,041 ns. Even the least unfavorable median, group 2 small/one-view edge, rises from 3,583 to 3,667 ns. Worst actual-control p95 is group 2 large/four-view cross-view move, 92,500 to 177,708 ns; worst max is group 1 medium/one-view resize, 9,958 to 80,292 ns. Favorable tails were also retained, including the 38,333 to 4,541 ns max for group 2 small/one-view center move.

Against ImGui, all p50 and p95 cells trail, and 33 max cells trail. The worst max ratio is 44.819188x for group 1 small/four-view cross-view move, 1,084 to 48,584 ns. The threshold remains 28/42 despite every actual-control median regressing; the threshold count is not an improvement criterion by itself, and every small cell still misses.

These observations support the recorded rejection of this exact split-plus-hints binary. They do not establish that either hint was honored, isolate one compiler/placement/register-pressure cause, assign an allocator/phase share, prove global overvalidation, or establish an architecture-wide lower bound. Source validity and unchanged work/output do not override the negative complete-operation result.

## Archive and restoration boundary

The archive is 415,948 bytes with 478 unique safe regular members: 477 inputs plus manifest. Every member's path, size and SHA-256 matches the manifest. All 477 inputs were independently roundtripped against frozen execution/original-package files or immutable source as appropriate: 424 execution/snapshot inputs, 45 original-package inputs, four actual-control inputs, three measured-candidate inputs and one combined source review. All six durable checksum entries match.

| Durable object | SHA-256 |
| --- | --- |
| Archive | `d39f7b771529e90fae11ba51a6bc4670288a89832ff077c5aab478a50aace92f` |
| Archive manifest | `af52dbfc0306e389df281057fbc57df3c1bf47866f4526e837867e3f54505fe9` |
| Comparisons | `f0b8fe996ed4fe9f34b25cbd1813c8389b6f000a43c1637f63bf774ea10d424a` |
| Decision | `7fa7bea58f9d97aaaed62abcc13286e93bfc612d11ed60a6835e06ecbca243c1` |
| Original and durable source review | `8d4b8f364e31bc9a4449d1d4b64e8adfa147f93588aa17b42247ab5679be19bd` |

The suffix-filtered original archive contains eight Rust base/tree pairs. The unchanged ninth `Cargo.toml` pair is hash/blob-bound in original metadata and receipts and was checked against the original package and immutable author parent; its duplicate file bodies are not falsely described as tar members.

The decision records rejection and exact reverse application of only the frozen three-path patch. Independently reversing that patch over the measured tree reconstructs all three actual-control files exactly. Those files and every one of the 324 actual-control source inputs match immutable restored `a53fac3` and control `37e388f`. This verifies the restoration result without relying on later live source or treating an unarchived shell command as observed.

The retained exact-total block is byte-identical in actual base, measured tree and restored immutable source. The two axis witnesses are absent after restoration because only this candidate's original +201 test lines were removed; existing test bytes and retained code remain. The ordinary inline hints and validation split were removed with this candidate. No later experiment is included in the rejection evidence, and this review makes no prediction about subsequent independent candidates.
