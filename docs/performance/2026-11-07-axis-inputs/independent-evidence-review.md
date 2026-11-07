# Independent evidence review: rejected committed axis-input experiment

**Findings first.** No blocking discrepancy was found in the archived measurements, qualification receipts, source binding, rejection decision, or exact restoration. The evidence supports rejecting this exact candidate: complete-operation p50 improves in 1 of 42 control comparisons and regresses in 41, with no ties. All fourteen 1024-pane group/cells regress by 11.75–24.47%. The rejected candidate reaches the ImGui 2x threshold in 26/42 comparisons; the restored control remains at 28/42. The overall performance target remains unmet and active.

One nonblocking wording clarification applies to the report at lines 36–37: `comparisons.csv` preserves all 84 aggregated group/cell comparisons, while the archive's raw CSVs preserve the 16,128 individual observations. The full evidence is present; the comparison table itself is not an observation-level table.

This review used immutable Git objects, archived bytes, and retained frozen artifacts as data. It did not run Cargo, compilation, formatting, tests, benchmark executables, comparison runners, or archived verification scripts. No repository, candidate, cache, or other report was modified. Qualification results below describe existing execution records, not new executions by this reviewer.

**Authority and artifact identity.** The reviewed checkpoint is `34f7b5f80469de13983bb029dac1a4d7a22406d5`, including `docs/performance/2026-11-07-axis-inputs.md` and its companion directory. The actual control is `07f566ac8fbddfab819f46f613d55bc2069507dd`. Live workspace changes are outside this candidate's identity.

| Artifact | Independently checked SHA256 |
| --- | --- |
| Performance report | `50ccc3108e684d3c517bf24d4a4d7a688722857a2a0551b0d2f2b3a681821ca5` |
| `evidence.tar.xz` | `f7ede7e75cc43ce92de58b1da5039c35e48d66179db3a9fbaeb1d6b95392b8e6` |
| Internal `manifest.json` | `be7acbc3d06dfba77486b594a350b72ba7430b41d73f19e67b4e7ba0388df7da` |
| `archive-check.json` | `92761efb09a1eee984e89c94d732ec13ece67929dc22a714b7764036d9af7c76` |
| `checks.json` | `33c9264e81340ef1f34a8d96350745d6d26691dbd483d8bd3752faf7d2ae90e4` |
| `comparisons.csv` | `ea5a1ee16986448b6f210bc9c8e60fa13323c3bd6fd0b1dd12658a15e9e3d030` |
| `decision.json` | `09d80cc515897abbff6ce76a9a54e9b9dd2e94e24ea82baf4cc62bbd4498738a` |

The 391,240-byte archive has 434 unique regular-file members: 433 inputs and its manifest. There are no absolute paths, parent-directory escapes, duplicate members, or non-file members. Every manifest size and SHA256 was independently recomputed. All eight entries in the committed directory's `SHA256SUMS` also match their immutable Git blobs.

All 433 original-byte bindings were checked: 428 frozen original inputs against their retained disk bytes, three actual-control inputs against immutable control Git objects, and two measured candidate sources against their frozen receipt hashes. The three source/design reviews agree across their archived, committed, and original temporary copies. Archive completeness therefore does not rely solely on trusting `archive-check.json`.

**Measured source and executable binding.** `axis-inputs-frozen/source-receipt.json` binds 324 source/configuration inputs. All 323 hashes in the ordinary verification receipt are an identical subset. Reading all corresponding control Git blobs establishes that exactly two source files differ from the actual control:

| Source | Actual control and restored checkpoint SHA256 | Measured candidate SHA256 |
| --- | --- | --- |
| `crates/dockspace/src/layout.rs` | `478cf5a1f3c6cf41cd3f21f3dfdc301a350957f18f58f900376edb256e38c05e` | `dc32cdecddbb742f5d6d225d48e80c2170facb385364c7deefc724e709fc367a` |
| `crates/dockspace/src/layout/topology_tests.rs` | `31c5e094b6c48cd58922de2642a08e223d1f5606f00463d1387e94a08ff93a2b` | `c02cc43844bc72a0b35186663c697e68b8838fc8f683fc8b80695f14b141974c` |

The archived actual-control sources equal `07f566a` byte for byte. Applying the archived runtime diff in memory reconstructs both measured candidate sources exactly. Its SHA256 is `ba8474c940ca456026ac74564c6f077797b95a8ca6703fd0ac12b371e3d83632`. The application receipt separately records the original author patch, SHA256 `8098b3d4c0a6aa190c1d31ef38fc01efec8ee6954fb5018316c06b441943ff03`; these are distinct patch artifacts and need not have identical bytes.

The frozen executables were read only as bytes and match their receipts:

| Product/artifact | SHA256 |
| --- | --- |
| `B/axis-inputs-frozen/dockspace_docking_cost` | `b361307359e199615dfb5d491187eb485f35a4652a03bd66732992c99b061c01` |
| Control: `B/unified-values-frozen/dockspace_docking_cost` | `270bbd69fd9856dddb8ea22ba0a9ed7edfa74a0b3e09a0f3101dac89f9a9300f` |
| `B/imgui_docking_cost` | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

Here `B` is `/tmp/dockspace-imgui-twofold-20261107-ea4161e`. Both comparison environments reproduce all 324 candidate source hashes and the executable identities. Their eight recorded run commands and successful return codes match the process outputs. The candidate build record contains one relevant compiler-artifact executable and a successful build-finished record, using optimization level 3, disabled debug assertions and overflow checks, the test harness, and default features. The source receipt records the locked, offline production build without execution, counter bypass enabled, and unchanged sources.

The archive does not embed the large executables. Its receipts plus the independently hashed retained frozen binaries provide the executable binding checked here; this is not a claim of a new reproducible build.

**Complete observation and output coverage.** The control and ImGui pairs each contain 8,064 raw rows. Each pair has two serial groups: ABBA and BAAB, where A is the reference product and B is the candidate. Each process records 48 samples after eight warmups for each of 21 cells, or 1,008 rows. This gives 96 observations per condition/group/cell and 16,128 raw observations overall.

Every process CSV has unique sample keys and complete grid coverage. Each aggregate raw CSV equals the recorded process rows with their group/run/condition fields. Product identity, panes, views, case, sample, nodes, leaves, splits, items, roots, and dimensions were checked throughout. Resize alternates widths 16,380 and 16,444; other operations use width 16,380, with height 16,380 throughout. All five phase durations are nonnegative, complete duration is positive, and the first four phases sum exactly to complete duration on every row.

All 336 complete-vector files were compared, covering 16 process runs times 21 cells. They agree byte for byte per cell across both reference pairs. Each contains the full expected number of u64 words:

`9 * (2 * panes - views + 2 * is_edge) + 2 * panes`.

Their FNV-1a digests were recomputed from little-endian u64 bytes and match the corresponding width-16,380 raw snapshots. Across all observations, the raw digest agrees within each of the 27 `(panes, views, case, width)` classes, including alternate resize widths. The 336 files are complete recorded cell snapshots, not one independent vector file per raw observation. That distinction does not weaken the actual archived coverage claim.

**Independent statistical recomputation.** The archived comparison script defines nearest-rank quantiles. For 96 observations, p50, p95, and max select sorted elements 48, 92, and 96 respectively. Using that definition, every statistic in the 168 summary rows across both pairs was recomputed for all five phases. Every old/new value, ratio, and reduction in all 84 comparison rows was also recomputed, as were the decision's aggregate statistics.

Reduction below is `(control - candidate) / control`; a negative value is a regression.

| Complete statistic vs actual control | Improvements | Regressions | Ties | Reduction range |
| --- | ---: | ---: | ---: | --- |
| p50 | 1 | 41 | 0 | −24.470852% to +1.537884% |
| p95 | 4 | 38 | 0 | −41.169104% to +41.908594% |
| max | 13 | 28 | 1 | −636.331016% to +88.643630% |

The sole p50 improvement is group 1, 16 panes, four views, cross-view move: 2,666 to 2,625 ns. Its p95 regresses from 2,959 to 3,000 ns, while max improves from 5,833 to 4,125 ns. All fourteen 1024-pane group/cells regress at p50, from 11.751256% to 24.470852%. The report's rounded values and rejection language are consistent with these calculations; neither tail regressions nor the single median improvement is hidden.

Against ImGui, all 42 p50 and all 42 p95 comparisons regress; max has 11 improvements and 31 regressions, with no ties. The reduction ranges are −337.181409% to −5.232285% for p50, −627.866667% to −1.847526% for p95, and −2272.266667% to +75.183521% for max.

| Panes | Complete p50 candidate / ImGui, both groups | At most 2x |
| --- | --- | ---: |
| 16 | 2.534400–4.371814x | 0/14 |
| 128 | 1.512000–2.012679x | 12/14 |
| 1024 | 1.052323–1.555151x | 14/14 |
| Total | | 26/42 |

The retained control's 28/42 result was independently checked against the immutable unified-values comparison table, rather than inferred from the axis candidate's results. This experiment moves the threshold result backward and does not satisfy the outstanding target.

**Qualification and logical work.** All twelve ordinary gate receipts have return code zero and agree with their logs:

| Existing ordinary gate | Archived result |
| --- | --- |
| Formatting | Successful receipt; empty log |
| Focused tests | 433 passed, 1,644 skipped |
| Fix/work regressions | 4 passed, 2,073 skipped |
| Core production | 2,074 passed, 3 skipped |
| Public integrations | 4 passed |
| Workspace | 2,250 passed, 3 skipped |
| Clippy | Correctness/suspicious deny checks passed |
| Documentation | Passed with `RUSTDOCFLAGS=-D warnings` |
| Doctests | 9 + 0 + 1 passed across recorded suites; no failures |
| Dependency boundary | Passed |
| Host default | 292 passed |
| Host serde | 335 passed |

Both added witnesses, `committed_axis_inputs_keep_infinite_extent_and_delayed_error_priority` and `raw_axis_inputs_keep_earlier_geometry_before_later_bad_weights`, have passing records in focused, core-production, and workspace logs. The invalid raw drafts are diagnostic fixtures, not additional valid-operation benchmark cohorts or evidence of unvalidated production workspaces.

The separate ordinary work command has a successful return code and one passing `docking_work` result. Its CSV has all 21 cells and 20 columns, SHA256 `88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4`. Completed root structures are one only for edge cells, totaling six; root-structure rows total 2,926. All values were checked, and the CSV equals the parent unified-values work CSV byte for byte. The parent archive checksum and its frozen source receipt were also checked before making that comparison.

Normal verification/work scripts remove the counter-bypass environment variable. Formal builds set it through the existing `DOCKSPACE_BENCH_NO_WORK_COUNTERS` gate in `drop_resolver.rs:1267–1275`, which omits test-ledger updates. This distinction is recorded, not silently treated as an ordinary work run. The work inventory is logical work evidence; it is not a count or attribution of allocations, bytes, or CPU time.

**Timer and numerical-work integrity.** The measured `docking_cost.rs` equals control, SHA256 `01d8438fc79095aacae935170aff3ed7eb8b5e497aa368c9966889b789adf2d3`. The projection, topology, and reference sources also remain identical to control. The full `solve_validated_axis_into` body and the desired-size/growth/shrinkage/redistribution arithmetic span are byte-identical. The change specializes private validation dispatch; it does not introduce a second numerical executor.

Source inspection confirms that complete time still includes request capture, checked mutation, fresh all-root leaf-answer collection and actual layout measurement, and fresh complete owned export. Complete output consumption and vector equality checks remain present. Fresh input/output work has not been moved outside the measured operation or replaced with cached results. Selected overrides and raw draft paths retain the original checked entry point; the committed path retains runtime extent validation at the solver position. The source reviews and archived error-priority witnesses support the stated boundary without turning those witnesses into a performance claim.

**Exact restoration and interpretation.** Both affected files at `34f7b5f` equal `07f566a` byte for byte, matching the restored-control hashes in `decision.json`. This removes the rejected runtime specialization and both added test functions from retained source while preserving them in the archive. The frozen candidate remains identified by its own source receipt and executable hash; subsequent workspace changes do not redefine it.

The report's retention decision is supported: reject this implementation and continue on the fully checked retained parent. The measurements do not establish an exclusive compiler, cache, branching, locality, or allocator cause. They do not establish that validation is free, that another implementation must regress, or that 28/42 is an architectural lower bound. No evidence-integrity or restoration issue blocks this rejection decision.
