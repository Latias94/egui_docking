# Persistent NodeStore: rejection evidence review

No material evidence or source-binding blocker was found. Immutable `9d669a425eb0f40f6e7483061168a1d0273ba98f` supports rejecting this exact persistent NodeStore and its dependency changes against actual control `37e388fbc95705204a42e4f8788623873c6ea711`. All 42 complete p50 cells regress by 67.75%–151.02%; the candidate meets the ImGui 2x threshold in zero cells. Correctness, output equality and fewer explicitly counted fork owners do not establish a complete-operation improvement.

This independently reviews `docs/performance/2026-11-07-persistent-store.md` and its evidence directory at that immutable commit. I parsed archived records and locked TOML data, recomputed statistics and complete-vector checks, reconstructed patches in memory and hashed frozen executables without executing them. No Cargo, compilation, tests, formatting, timing, runner, archived script, profiler, network or agent was run. No repository, fork, cache or candidate file was written; only this report was written. Root owns the recorded execution.

## Original parent, actual source and build binding

The original author parent is `a2365aa876c51a66a5c9c788bff9056afdbabacd`. Actual control is the later retained exact-total `37e388f`. All 17 original source pairs and their metadata match the author parent. The six modified files and five applicable baseline locks remain byte-identical between that parent and actual control, allowing the original six-path patch to replay exactly. Unmodified original context, including the older layout, is not substituted for the actual retained layout.

The original patch reconstructs the six resolved source/manifest files exactly. Cargo supplies the five resolved lockfiles; the original source-only package deliberately supplies no candidate locks. The only subsequent formatting change is two wraps in the newly added `copy_tests.rs` code: collapsing one single-argument call, including its redundant trailing argument comma, and wrapping one `let retry` assignment. The exact resolved-to-formatted snapshots differ only by those two changes. No runtime expression, test assertion or dependency fact changes during formatting.

| Bound object | SHA-256 |
| --- | --- |
| Original six-path patch, 17,636 bytes | `7d114b534dae5cb2a8b4ccbb43f9b9663a8e7ef402cbe0cea768694853cf8882` |
| Actual frozen eleven-path patch, 25,577 bytes | `cc61a7cd4eba27d2dcaa3ee6da7ba304ac76821c36ff1d9ebd4e6bd3c026d0ef` |
| Original manifest | `88304ab7ab56de70dbf81372cfb6d619df3594079a9132d678ddcd51856cb0c7` |
| Original READY | `73448bdd3dc71bd24303ad83269784f0d3c8893aea6fb5c6ca1a9aefd77d1f43` |
| Application receipt | `cb6212dc5e1b32fe43882b160b6a4cf5730184b63b7b1ee6aceb502e7b74bfd1` |
| Actual source/build receipt | `c4d20e835b2455ebc961af0c3315d29eeb555c7bacfb1afebd6cc1cb37c1f647` |
| Ordinary-check source receipt | `22943fee810ab431ce8b40e7b1f928379cd2e86f2ef6a634b733c1d8a0d251dc` |
| Formatting patch | `dda816eb587abaf83cf205d609744534479fc10276e3481af3060d25f8581357` |
| Candidate executable | `0a48c0edeaad9185ab85efd0e2671247d72026c24a7aed37e31bd8b0a7758b45` |
| Actual exact-total control executable | `ccbc1be2fe9d0c14b63e8fbed845fda6332148786485c539b9166d6efdcca5f3` |
| ImGui executable | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

The actual patch is +385/-26 across 11 paths and 31 hunks: the original six edited files, measured after formatting, plus five real lock changes. Its exact forward reconstruction equals every measured/formatted snapshot; its inverse reconstructs every actual-base file exactly. The original patch, actual patch and formatting patch describe different stages and are not presented as interchangeable bytes.

All 328 candidate receipt inputs match the actual control plus exactly those 11 measured files. All 327 ordinary-check inputs match that candidate receipt; the extra formal input is the unchanged C++ comparator. All 324 original control receipt inputs match immutable `37e388f` and restored `9d669a4`; the four extra integration locks in the candidate receipt were independently checked against immutable control and resolved snapshots. Both actual and historical source-set identities are retained. Original source-only/lock-pending metadata describes preparation, not the later executed qualification.

The source treatment remains the reviewed `Arc<SlotMap<NodeId, ()>>` allocator plus one canonical `HashTrieMapSync<NodeId, Arc<Node>>` value directory. Its complete generational keys, slot-order iteration, retain order, owned removal return and 13 private interfaces are bound to the original reviewed source. The measured layout keeps packed results and exact-total reuse. No axis split/hints, SmallVec, fused continuation or binary gather is mixed in.

The whole formal timed portion of `docking_cost.rs` remains unchanged. Its delta is confined to the untimed ordinary work probe's renamed column/access and explanatory comments. This experiment keeps request capture, checked mutation, fresh all-root layout and complete fresh owned export.

Hash-only reads of all frozen binaries agree with their receipts: candidate 18,676,256 bytes; actual control 18,734,208 bytes; ImGui 503,280 bytes. Both Rust build records select one dockspace executable and finish successfully with optimization 3, debug information 0, debug assertions false and overflow checks false. Normal checks/work remove `DOCKSPACE_BENCH_NO_WORK_COUNTERS`; formal builds set it to 1. The existing unused `presentation_identity_frontier` warning remains, so this is not called warning-free.

## Actual dependency resolution and retained failures

All five applicable locks have real Cargo resolution records and complete actual-base/resolved/formatted snapshots. I independently parsed those snapshots with Python 3.13 standard-library `tomllib`, without Cargo or network. The resulting parsed facts and full old package stanzas agree with the archived lock check.

Only these three packages are added in every lock:

| Added package | Version | Checksum |
| --- | --- | --- |
| archery | 1.2.3 | `33ca55ee147b1926dbea904f50fe4902494e97bc742205abbbf10c709e43815f` |
| rpds | 1.2.1 | `e025feb26210bc196b908e72deb063b1b4000754304341cbc168a1e72c857ebc` |
| triomphe | 0.1.17 | `0d15f4f5898bcfc5d8a8e45ed30b2f108d710c1a2e5489dbf80e078f707181bc` |

The locked edges are dockspace → rpds, rpds → archery + existing smallvec, archery → triomphe. Smallvec remains 1.16.4 with its existing stanza unchanged. Every pre-existing package version, source, checksum, dependency list and full stanza is byte-identical except dockspace's added rpds edge. Removing just that edge reproduces its original parsed package. No old package is removed or replaced, and top-level lock metadata is unchanged.

| Applicable lock | Before packages | Resolved packages |
| --- | ---: | ---: |
| workspace | 323 | 326 |
| dockspace-host-harness | 46 | 49 |
| egui-fork-workspace | 314 | 317 |
| egui-official-harness | 338 | 341 |
| egui-product-harness | 87 | 90 |

The resolution history contains six runs, return codes `0, 101, 0, 0, 0, 0`, with serial intervals. The first workspace command succeeds. The first dependency-selected host tree command includes `--all-features`, then errors with “cannot specify features for packages outside of workspace”. Its stderr also shows that Cargo had already locked the three new packages before failing the tree feature selection; it must not be described as a no-mutation attempt. The initial receipt and stdout/stderr are retained.

The four corrected dependency-selected commands remove that workspace-only option, preserve the normal/build tree selection, and succeed. Together with the workspace success they cover all five manifests; their tree output explicitly contains the locked rpds/archery/triomphe versions. Successful resolution receipt hash is `8edc4d04bd27b768360d3cea5d09554350090212978bf14172cf05a7be70aa04`; lock-check hash is `52dadd5752ba4cae08793968ec36161af2db9d17e8702f65db6c08c39cd7cfb3`.

The application receipt records the separate Python 3.9 `tomllib` import failure before lock inspection/mutation and correction to Python 3.13. The archive preserves that note and the completed parsed lock check, not a separate raw Python traceback; I do not claim otherwise. This review's independent Python 3.13 parsing confirms the final results. No new dependency resolution was attempted here.

Both historical title-resource allocation evidence locks remain byte-identical to author parent, actual control and restored immutable result. Their shared hash is `2a82f1423b6d87f5a941de8fe002525c7b51563656fa3e8e34c3171838d2596b`. The resolved build-lock edits are not confused with those historical artifacts.

## Gates and the explicitly changed work metric

All 12 ordinary gates have return code zero and serial receipt intervals. Every numbered nextest PASS entry agrees with its full summary; there is no anchored FAIL/TIMEOUT/ABORT/CANCELLED record.

| Gate | Passed | Skipped |
| --- | ---: | ---: |
| targeted | 434 | 1,644 |
| fixes | 4 | 2,074 |
| core-production, all features | 2,075 | 3 |
| public integrations, no default features | 4 | 0 |
| workspace, all features/all targets | 2,251 | 3 |
| host default | 292 | 0 |
| host serde | 335 | 0 |

The remaining gates are fmt, clippy, strict docs, doctests and the core dependency boundary. Fmt has an empty successful log; clippy retains its correctness/suspicious policy. Docs use `RUSTDOCFLAGS=-D warnings`. Doctests have 9 core, 0 conformance and 1 egui passes with zero failures. The normal/build all-features/all-targets dependency boundary passes and includes the three new packages. These gate inventories overlap.

The production log explicitly records both new semantic witnesses passing: branch-local allocation with late-failure retry and dense/sparse churn preserving arena order, retain calls and free head. It also records the renamed directory-owner fork witness. Existing numerical/layout oracle, MRU, ownership, old-output and staged-candidate contracts remain in the full source-bound suites. The two formatting wraps do not weaken those assertions.

The separate work probe has one pass and 2,077 skipped tests. Its CSV still has exactly 21 rows and 20 columns, with exact original row order. Nineteen columns equal actual control field for field. Column `node_handle_clones` is intentionally replaced at the same position by `node_store_owner_clones`; the whole CSV is therefore not equal to control.

| Work CSV | SHA-256 |
| --- | --- |
| persistent candidate | `cceb7529730bab697009bed4265647c56b126b2faadd548ba41ed44fd921cbd9` |
| actual exact-total control | `88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4` |

For all 21 rows, the new value is exactly `2 * node_store_forks`. The source records that before the allocator Arc and persistent directory root clones. Resize has zero forks/owners; each command row has one fork and two explicit root-owner clones. All unchanged logical inventory, actual payload/tab/MRU/split-element counts were independently checked against the deterministic formulas and actual control. Six completed bodies and 2,926 root rows remain.

This is a metric-definition change, not a subtraction of total copies from N to 2. It excludes rpds path/shared-pointer/Entry copies, payload Arc copies inside persistent entry COW, the owned-remove return handle clone, allocator COW metadata including historical holes/free-list storage, allocations/bytes, and other Workspace directories. The old cloned-node/tab-item columns remain logical source inventory. No zero-N total work or allocator benefit is inferred from the new counter.

## Observations, complete outputs and recomputed statistics

Both campaigns use serial ABBA followed by BAAB, with 48 samples/eight warmups per process/cell and 96 observations per condition/group/cell. All 16 processes succeed, recorded intervals do not overlap, and all stderr captures are empty. Rust stdout records the selected probe passing; all four ImGui stdout captures equal their process CSVs.

Each process contains 21 complete cells and 1,008 observations: panes 16/128/1024, views one/four, resize/center/edge, with cross-view moves at four views. Each campaign contains 8,064 raw rows; together they contain exactly 16,128. Aggregate CSVs exactly reproduce individual process rows. Product, full sample/cell sets, topology/item/root counts, resize widths and nonnegative times all match the protocol. Complete-operation time equals the sum of request, mutation, layout and export for every row.

All 2,520 phase/stat summary fields were recomputed at nearest-rank p50/p95/max indices 47/91/95. All 1,008 numerical comparison fields across 84 rows match those summaries and their exact ratio/reduction formulas. All document and decision statistics agree.

All 336 complete vector captures were parsed and checked for complete unique preorder topology, expected node/leaf/split counts, root and child geometry, four-unit split gaps, selected-item membership and every item exactly once. Their little-endian-u64 FNV digests match sample-zero raw records. All 21 cell vectors are byte-identical across products, conditions, runs and campaigns. Every raw digest agrees at each of the 27 cell/actual-width keys, including both resize widths. These checks cover every archived complete vector and raw digest, not an unrecorded full vector for each raw sample.

Positive reduction denotes faster performance; all unfavorable and favorable observations remain included.

| Reference | Statistic | Improve / regress / tie | Reduction range |
| --- | --- | --- | --- |
| actual `37e388f` control | p50 | 0 / 42 / 0 | -151.018251% to -67.751118% |
| actual `37e388f` control | p95 | 1 / 41 / 0 | -245.528227% to +12.654545% |
| actual `37e388f` control | max | 7 / 35 / 0 | -537.674089% to +76.994706% |
| ImGui | p50 | 0 / 42 / 0 | -580.000000% to -102.679664% |
| ImGui | p95 | 0 / 42 / 0 | -668.181818% to -27.154607% |
| ImGui | max | 5 / 37 / 0 | -6032.686981% to +53.494960% |

| Panes | Actual-control p50 regressions, all 14 cells | Candidate / ImGui p50 | At most 2x ImGui |
| --- | --- | --- | ---: |
| 16 | 67.751118%–98.785185% | 3.867200–6.800000x | 0/14 |
| 128 | 86.397912%–111.316247% | 2.510040–3.178571x | 0/14 |
| 1024 | 130.270177%–151.018251% | 2.026797–2.786191x | 0/14 |

The least unfavorable actual-control median is group 1 small/four-view cross-view move, 2,459 to 4,125 ns; the worst is group 2 large/four-view resize, 77,584 to 194,750 ns. All 12 resize medians regress, by 69.230769%–151.018251%, despite no candidate fork savings on that path.

The worst actual-control p95 is group 2 large/four-view center move, 88,958 to 307,375 ns. The worst max is group 2 medium/four-view resize, 12,709 to 81,042 ns. Favorable tails are retained, including group 1 small/one-view edge max, 48,541 to 11,167 ns. Against ImGui, the worst max is group 1 small/one-view center, 1,083 to 66,417 ns; the favorable group 2 large/one-view center max, 585,958 to 272,500 ns, also remains.

Candidate target attainment is 0/42; actual retained exact-total stays 28/42. This rejects the measured read/update/ownership tradeoff as a whole. It does not isolate hashing, path allocation, reference counting, final-owner destruction or one allocator share, and does not establish that every persistent representation would fail. Reduced explicitly counted fork-owner work cannot erase the observed whole-operation regressions.

## Archive completeness, exact restoration and native preservation

The 605,844-byte archive contains 545 unique safe regular members: 544 inputs plus manifest. All paths, sizes and SHA-256 values agree with that manifest. Each input was independently roundtripped to frozen/original files or immutable source as appropriate: 467 execution/resolution inputs, 53 original-package inputs, 12 actual-control inputs, 11 measured-candidate inputs and one combined source review. All six durable SHA256SUMS entries match.

| Durable object | SHA-256 |
| --- | --- |
| Archive | `9d07102cc50c156269f8d65a23db8777ec4563639a40368a3da66c30f33d732d` |
| Archive manifest | `2c4a8de6cf383be30ea0a1737be76f1f0f79a0fecc38d0f15d34db2f5d30499b` |
| Comparisons | `2402d15b557081d2635628a94ecaf292fd42bdfb2fea3f153599a78e3e2c0c1c` |
| Decision | `5e6eb3ceb9ed0a824456032f26e5adcb5f7abd1e15771c3162677c111342e23d` |
| Original and durable source review | `55ac2e3489cee9a8f5fd2995194635318a23cbcc7b9d6f194bf7a0087ac2c069` |

Original source-set hashes are base `eebd88b526b1411ed121d9a6fe5eddb9f72848e14ae66ca4c19bc5e8860bf7d0` and tree `d06852f0f07c22105a6a9e63f0ecf589543923085fc7db8a81189e3b556e821c`. The suffix-filtered original package preserves 15 Rust base/tree pairs. Its two TOML bodies, original rpds `.crate` input and upstream rpds `Cargo.toml` are not duplicate members under `original-candidate/`; their archived metadata hashes were checked against the original package, with immutable Git also supplying the two project TOMLs. Actual project before/resolved/formatted TOMLs and all five locks are present in the resolution snapshots, so the measured source/lock transformation is completely preserved. No omitted original binary or TOML is described as an archived member.

The exact inverse of the frozen patch restores these 11 paths: workspace `Cargo.toml`/`Cargo.lock`, core `Cargo.toml`, `docking_cost.rs`, `drop_resolver.rs`, `graph/copy_tests.rs`, `graph/shared_storage.rs`, and the host/fork/official/product harness locks. Their restored bytes match actual control `37e388f`, the decision hashes and immutable `9d669a4`. The retained exact-total layout is outside the patch and remains control-identical. All 328 candidate source-input paths, evaluated at the restored immutable commit, match actual control bytes after removal of this experiment.

The native motion-receipt fix is separately retained. Its four smoke files at `9d669a4` are byte-identical to immutable native-fix commit `c1ed518992d0cd88026960940ea061aca9fb0433`; those paths are absent from the persistent patch. In particular, the restored fork lock removes only the persistent dependency closure and returns to its baseline bytes; it does not roll back the native source fix. No physical-native success is inferred from this core performance review.

The rejection therefore restores the measured core/dependency control precisely while preserving unrelated retained native work. Later live binary-gather work is excluded from every source, executable and restoration comparison above.
