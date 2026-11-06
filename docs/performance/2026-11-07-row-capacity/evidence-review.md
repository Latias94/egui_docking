Findings first: **no evidence or retention-judgment blocker found** for the row-capacity checkpoint. The report accurately describes a bounded edge improvement with complete-operation regressions, retains all negative p50/p95/max comparisons, and leaves the 2x objective incomplete at 28/42. Its header/capacity discussion makes no unsupported enum-size, allocator-byte, peak-memory or allocator-time claim. No source fix is requested or authorized by this review.

This review binds immutable candidate `e3c517195ddce2c7d1a6dbfd4328f152978968ec` to actual control `3dd308cc71a62184af31203b1119db11e5b504ed`. All seven report/evidence files under `docs/performance/2026-11-07-row-capacity.md` and its directory match the candidate commit exactly. The current slab WIP was not used as the candidate. The report SHA256 is `4a7818ecc7578709dc5393623a2011f9ac11ae35796b632485ef56ed7d761199`.

Only `/tmp/row-capacity-evidence-review.md` was written for this task. I read immutable Git blobs and frozen/archive receipts, hashed and compared data, and independently recomputed recorded statistics. I did not run Cargo, compilation, tests, benchmarks, timers, archived runners, or delegation, and did not modify repository, fork, cache, candidate or archive files. Passing-check counts below describe Root's recorded executions.

**Immutable source and executable binding.**

The [report's patch and cost statement](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-07-row-capacity.md:13) binds original patch SHA256 `13a67cd74cddc850dbd8064725ddd90bc7d57efbeb4eb93efe79575aff5d78e6` (6,205 bytes, +42/-13). I reproduced that patch byte-for-byte using the complete archived base/candidate source pairs. The frozen Git-format `row-capacity-frozen/runtime.patch` is SHA256 `2907221d828e88f502889aacf504b281da9ddefe457d0536ba9729ab27454f07` and equals the immutable control-to-candidate core diff byte-for-byte. Git metadata/function context accounts for the two patch formats.

| Source, relative to `crates/dockspace/src/` | Control SHA256 | Candidate SHA256 | Diff |
|---|---|---|---|
| `graph/authority.rs` | `89c65b0661ae3cc31b38b0157d4767e77c68219186ce78ca784ffb44d6af3fb9` | `89d0d337cdc2365caf62f5dd2d48d6f9132b21c2d4b882e9c24096cc6602a600` | +18/-11 |
| `graph/root_structure.rs` | `a4746315ca204bad7759db02eaf9d95488b6169ec873dab6207766d669b8e710` | `e80a74d1bdef525bd25a9a627bd1c826f41cd1def8fe05583dcea14372938632` | +24/-2 |

These pairs equal the corresponding immutable Git blobs. I also matched every source entry in both frozen receipts against its corresponding commit: 324/324 for the Item-frontier control and 324/324 for the row-capacity candidate. Their sets agree and exactly the two paths above differ. Every one of the 323 ordinary-qualification source/manifest hashes agrees with the formal candidate receipt; the latter adds the pinned ImGui comparison source. This establishes the measured source combination without using live core files.

The candidate and actual-control frozen executables were rehashed from their existing frozen paths, matching [the report](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-07-row-capacity.md:30) and environment receipts:

| Frozen executable | SHA256 |
|---|---|
| Candidate | `be863f934c761071b9ebc206a0a328941f9d8a12f75e2cb9a8119470e0784a6e` |
| Actual control | `09a32eeefd9e3781133c6662cf917d97b5277a372e748a339425a6534313dfe9` |
| Reused pinned ImGui | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

The ImGui receipt retains pin `d88f238d1d363ef387382b2e29ad12cf96f826ac`. The archive stores source pairs, receipts and build/run outputs; executable identity is also independently confirmed against the existing frozen files.

**Header and reservation costs are represented accurately.**

The archived candidate `graph/authority.rs:72-115` changes the unit Editing variant to `Editing { previous_nodes: usize }`. First begin copies only the previous Ready body's `nodes.len()` and replaces that owner; it retains no additional RootStructure Arc. Repeated begin preserves the prior count. `take_root_structures` installs zero while moving the existing Ready Arcs (`:120-133`); general completion still invokes the full builder (`:143-189`); a newly introduced owner starts with zero (`:349-355`). Only Item rows completion passes the count to `build_after_item_move` (`:161-171`). These source facts support the report's allocation-hint scope and the linked [independent source review](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-07-row-capacity/review.md).

The archived candidate `graph/root_structure.rs:92-114` passes zero on the full placement-installing builder and derives the Item hint using `previous_nodes.saturating_add(2)`. At `:140-160`, only after the first actual node lookup succeeds, a final Split requests h node/postorder/ordered directories, h-1 child edges and floor((h-1)/2) split rows. A final Tabs root requests one entry in each node directory and makes no old-size child/split reservation. The original pushes, traversal, directory rebuilding and pending stack remain. A hint does not set initialized length or terminate traversal. Those are element reservation requests; Vec can allocate beyond a request.

A scalar-carrying enum may lose the previous Ready-Arc/unit-Editing niche and enlarge RootStructureState/RootOwner, including while Ready. Copying the root-owner BTreeMap could then cost more. First begin also reads the count on non-Item paths, and a shrinking Split can publish unused reserved capacity. The source does not establish actual type layout, exact capacity, allocator bytes, peak residency or exclusive allocation latency. [Report lines 15-18](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-07-row-capacity.md:15) and the archived review preserve these uncertainties. [Lines 46-49](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-07-row-capacity.md:46) correctly decline to attribute measured regressions to the possible larger header. No invented sizeof or allocator saving was found.

**Archive integrity, complete work and qualification receipts.**

The [archive](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-07-row-capacity/evidence.tar.xz) is 349,356 bytes, SHA256 `ae4b44c59da0864dc131d4f03d4364f46e3f56638311840cbf01b6809d041062`. It contains exactly 431 unique, regular members with safe relative names: 430 input files plus `manifest.json`. All 430 manifest byte lengths and SHA256 entries independently match the member bytes. I compared every input against its corresponding original origin or immutable Git source: 418 frozen/verified/script files, seven original-author evidence files, two control sources, two candidate sources and the control-commit text. All 430 round-trip comparisons agree. All five entries in [SHA256SUMS](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-07-row-capacity/SHA256SUMS) match. The public archive-check and checks summaries accurately describe these retained inputs.

The 12 ordinary checks in archived `row-capacity-verified/checks-all.json` each have exit zero and agree with [checks.json](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-07-row-capacity/checks.json). Log summaries confirm 428 focused tests, four selected work regressions, 2,069 production core tests, 2,245 workspace tests, four public integration tests and 292/335 default/serde host tests. Production/workspace retain the same three skipped opt-in probes. The remaining fmt, correctness/suspicious Clippy, strict-docs, doctest and dependency receipts report success. The archived verification script explicitly removes `DOCKSPACE_BENCH_NO_WORK_COUNTERS`; the formal freeze separately sets it to 1. Ordinary logical-work evidence is therefore kept distinct from the timing executable.

The work probe's separate receipt is also exit zero. Its CSV is byte-identical to `item-frontier-verified/work.csv` in the control's archived evidence, SHA256 `88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4`. I verified all 21 cells and all 20 columns, not just the work-summary boolean. Six successful root bodies contain 2,926 rows and 1,460 split handles. Only edge cases complete a body, with rows 33/9, 257/65 and 2049/513 for one/four views respectively. Resize keeps zero mutation-copy work; Center/cross-view MRU copies remain four and edge MRU copies two. Fingerprint and item-multiset scan columns remain zero. All other existing copy columns also agree with the archived control because the complete CSV bytes agree. This supports [report lines 25-28](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-07-row-capacity.md:25). These counters measure successful logical row/payload work and contain no Vec-capacity or allocation-byte counter; equality cannot prove an allocation saving or absence of extra reserved capacity.

**Complete protocol and independent raw/statistic checks.**

Each comparison retains both ABBA and BAAB groups, eight processes total, 48 recorded samples after eight warmups per process/cell, fixed 16/128/1024 panes and one/four views, with cross-view only at four views. All 16 process run receipts across the two comparisons have exit zero. I verified exact process CSV headers and their aggregate reconstruction, no missing/duplicate sample keys, each 1,008-row process grid, expected node/leaf/split/item/root counts, alternating resize widths, nonnegative phase durations, exact phase-total sums, and valid digests. Each group/condition/cell has 96 observations. Thus the two matrices contain 8,064 observations each, 16,128 total.

I checked all 336 complete vector files: 21 cells for each of 16 processes. Full bytes agree by cell across both products, both comparison pairs and all groups. Record framing, complete item inventory/counts and sample-zero vector digests match their CSV records. Across all raw observations, digests also agree by cell and input width, including both resize widths. The full word vectors have not been replaced by digest-only evidence.

I independently recomputed all five phase statistics in both 84-row summary tables using the unchanged nearest-rank percentile rule (p50 is sorted observation 48 of 96; p95 is 92 of 96). All summary values agree. I then checked all 84 public comparison rows, for all p50/p95/max input values, ratios and reduction percentages directly against the raw data. No cell or adverse statistic was omitted.

The complete operation remains the interval from request capture through checked mutation, fresh all-root projection and owned export. Immutable `docking_cost.rs:441-468` retains those five timer boundaries; fixture creation is before measurement (`:794-795`), and validation, full expected-word comparison and destruction of owned outputs remain after it (`:796-820`). `export_state` retains complete preorder export and verifies there are no leftover nodes (`:329-350`). This review confirmed these protocol files are byte-identical between the two commits:

| Unchanged source | SHA256 |
|---|---|
| `crates/dockspace/src/docking_cost.rs` | `01d8438fc79095aacae935170aff3ed7eb8b5e497aa368c9966889b789adf2d3` |
| `scripts/compare_docking.py` | `6a099e1f5fdc48da67e730576ec59baff9433d007c01e0f183f96e3c6146ffa5` |
| `scripts/compare_products.py` | `226401f5a4569acd64b6cd8cabc47fe0318e4b9d86755c9f7ae5d17777bb14e7` |
| `integration/product-comparison/imgui_docking_cost.cpp` | `4c74fbb088618dff1c71d4eaaa4094889c4516d4ecef3fd2342eeeb9c855f36c` |

**All comparison conclusions and the retention decision.**

Reduction means `(1 - candidate / reference) * 100`; a negative is a regression. Recalculation gives:

| Reference | Statistic | Improvements | Regressions | Ties | Reduction range |
|---|---|---:|---:|---:|---:|
| Actual control | p50 | 27 | 15 | 0 | -10.2116% to +11.0151% |
| Actual control | p95 | 23 | 17 | 2 | -11.0169% to +17.2414% |
| Actual control | max | 17 | 25 | 0 | -407.9365% to +85.8562% |
| ImGui | p50 | 0 | 42 | 0 | -386.7200% to -2.1329% |
| ImGui | p95 | 3 | 39 | 0 | -449.7751% to +18.6618% |
| ImGui | max | 12 | 30 | 0 | -2988.8000% to +72.9740% |

The full [comparisons.csv](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-07-row-capacity/comparisons.csv) retains all these negatives, including every p95/max outlier. [The report's rounded counts/ranges](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-07-row-capacity.md:39) match. Specifically, the edge p50 reductions are 8.0727–11.0151% at 16 panes and 5.4417–9.1312% at 128. At 1024 they are 3.4985–4.4287% for one view and +2.2724%/-0.1114% for four views. The single-view 16-pane Center regressions are -3.1619% and -10.2116%. All four large Center group/cells regress, as do three of the four large resize group/cells. These observations support a qualified edge tradeoff, not uniform acceleration or a demonstrated allocator cause.

| Total panes | Candidate / ImGui p50 range, both groups | At most 2x |
|---|---|---:|
| 16 | 2.5344–4.8672x | 0/14 |
| 128 | 1.511999–1.922248x | 14/14 |
| 1024 | 1.021329–1.402288x | 14/14 |

The [goal statement](/Users/frankorz/codes/rust/egui_docking/docs/performance/2026-11-07-row-capacity.md:53) correctly leaves the total at 28/42. Every small group/cell still exceeds 2x; every candidate p50 still trails ImGui. Retaining this slice for measured edge improvements while disclosing Center/resize regressions and uncertain header/capacity costs is supported by the preserved evidence. This review supplies no independent execution or additional performance measurement and does not mark the overall objective complete.
