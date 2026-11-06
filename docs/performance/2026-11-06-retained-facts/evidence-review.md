# Final evidence review: retained surface facts

Reviewed at 2026-11-06T12:25:58.292611+00:00. Source snapshot: `4d874443ffdae2220c70401fe6f46335d004823983887dd09b6e0bc13dcfbc20`.

Status: **approved for the measured plan/goal qualification; no blocking evidence finding remains**. This replaces the preliminary review. The 10% figure is the implementation owner's chosen investment/acceptance threshold in the plan and goal, not a numeric requirement supplied by the user.

This lane only read existing source, documents, logs, CSVs, JSON receipts, compressed archive files and executable bytes. Inline arithmetic/hash checks did not execute a measurement binary, Cargo, tests, benchmarks, profilers or network requests. No repository or Git state was written. The only write is this report. Test results below are inspected owner-run receipts, not tests independently rerun by this reviewer.

The reviewed result document is `docs/performance/2026-11-06-retained-facts.md`, SHA-256 `bb674d9a3510a78f7e2942fbc4126a1643089198c1f68daf7a744ef47a1e433c`. Its measured claims and qualifications match the inspected evidence. Its remaining “final evidence review in progress” status can be updated after receiving this report.

## Qualification verdict

The two formal product groups each pass the plan's >=10% reduction gate for the original complete 1,024-pane press-plus-release click. The calculation is `100 * (before_p50 - after_p50) / before_p50`, separately in each group:

| Formal group | Baseline p50 | Candidate p50 | Recomputed reduction |
| --- | ---: | ---: | ---: |
| 1, ABBA | 25,222,749 ns | 16,681,041 ns | 33.865095355% |
| 2, BAAB | 25,072,583 ns | 16,685,166 ns | 33.452544558% |

The reported 33.87% and 33.45% are correct rounding. There is no cross-group averaging used to rescue a failing group. At every pane count and in each headless group, both candidate selection-update process p50s are below both baseline process p50s. The required same-direction complete headless result is therefore supported without inventing pooled raw headless percentiles.

## Source, baseline and protected state

I independently checked:

- The current discovered Rust/manifest/lock/toolchain input set is exactly the recorded **565 paths**, with no added or missing relevant path. Every current content hash matches `state.json`, `validated-source.json` and the candidate build's `dockspace_sources` table.
- The SHA-256 of `validated-source.json` is the stated snapshot. Reconstructing it as `json.dumps(state["source_sha256"], indent=2) + "\n"` also reproduces that digest, as the archive README explains.
- The complete baseline-to-final content delta is exactly the recorded **41 paths**, equal to both `state.source_scope` and `frozen-product.environment.source_changes`.
- All **49 protected user CI/review files** retain their starting hashes. `repo-ref/egui` is outside implementation ownership and is not the product's local path dependency.
- All eleven successful check receipts name this exact final snapshot. Their commands equal the eleven declared commands. The review receipt names the same snapshot, has zero blocking findings and references two report hashes that match their actual contents.
- The final fixture migration is documented honestly: the earlier 2,185/2,186 workspace failure is retained under `pre-fixture-migration-*`; the final successful set includes the two corrected synthetic fixtures. That migration did not change production/workload inputs. The failed set is not presented as successful final verification.
- All three benchmark-script/workload hashes and all ten pinned ImGui source/header hashes match baseline and current files. The complete product and headless workloads were not edited to obtain the result.
- All recorded result hashes in the formal product, headless and candidate/reference environments match actual CSV bytes. All four frozen executable hashes match their receipts.

The baseline is supported by content provenance rather than a misleading clean-checkout label. Its 565 hashes exactly equal the prior qualified product's recorded built-input table. The prior environment's Git label is `2935571...`; the baseline receipt names base `34ab23adf8062960298abbc5fbcba370104f3b86`, runtime `54840717a2454c957b13a667a939ddf8229e52e9`, and records the already-qualified matching executable. The result document correctly says this is reused qualified binary content, not a fresh rebuild of clean `34ab23a`.

| Frozen executable | SHA-256 |
| --- | --- |
| Baseline complete product | `c00a38ed2552d942dd99048eb06c491d27b0881e41efc122c12d148e7ddfb518` |
| Candidate complete product | `1d928f145e8396f207f110043bbeac4d8e0a37eb73ef40f28dc9f3f6a94cb9f0` |
| Baseline complete headless | `444f05f82da82ca495604fd3ca1b2f050cd390d88741167e899e0235131ca77b` |
| Candidate complete headless | `17f80f8cae35ced686f6e705b51a2103171964fd9aca3a5dfcd69b2514fd4a95` |

Baseline and candidate product receipts agree on compiler versions, platform/machine, manifest/backend, production profile, complete build commands and pinned ImGui commit. The actual egui Git dependency is manifest/lock-pinned to `60c40304dab75305d2ac0f54a1cabfc354669006`, built `--locked --offline`. Product production uses ThinLTO/one codegen unit. Prior and final headless build environments exactly agree on one Cargo job, release ThinLTO and one codegen unit.

The qualification entry gate now explicitly enforces check-snapshot and review-snapshot equality with the state, resolving both preliminary receipt concerns. Entry/exit hash checks and the final independent audit support the exact-source claim without adding a Rust proof script.

## Complete product raw rows and arithmetic

The formal raw table has **1,728 rows**: eight processes × three pane counts × three cases × 24 samples. Each process's original CSV equals its portion of the concatenated raw table. All expected sample identities are present exactly once. Every group/condition/case contains 48 raw samples.

For every row I checked the two-tabs invariant, original one/two-frame case size, actual passes >= frames, visible contents = panes × actual passes, content calls >= visible contents, positive elapsed time and nonempty vertices/indices/draw commands. For every matched pane/case/sample identity, the tuple of **frames, passes, content_calls, visible_contents, vertices, indices, draw_commands** is exactly equal across all groups, conditions and runs. No avoided content or mesh work explains the gain.

All **36** formal p50/p95/max rows recompute exactly from their raw elapsed values using nearest rank `sorted(values)[ceil(n * percent / 100) - 1]`. All nine product-table rows and both reduction columns in the result document match those summaries and the recomputed arithmetic.

| Panes | Case | Group 1 reduction | Group 2 reduction |
| --- | --- | ---: | ---: |
| 16 | idle | 2.681849023% | 2.949527779% |
| 16 | tab_click | 24.504424829% | 23.271641220% |
| 16 | surface_resize | 2.587264744% | -1.272618910% |
| 128 | idle | 5.785729831% | 4.650080360% |
| 128 | tab_click | 31.647130376% | 30.389077972% |
| 128 | surface_resize | -0.786018072% | -1.055774206% |
| 1024 | idle | 7.106202209% | 3.575301092% |
| 1024 | tab_click | 33.865095355% | 33.452544558% |
| 1024 | surface_resize | 0.575082474% | 0.882112189% |

The unchanged source confirms that a click is the sum of actual press and release frame timers. Each timer includes RawInput preparation, owned egui execution, real selected pane text/square content, CPU tessellation and Presented reporting. Geometry discovery and hover preparation precede those timers; post-frame validation, mesh inspection and disposal are outside them. This is complete CPU draw-data generation, not GPU/native-window/vsync or input-to-display latency.

The negative resize cells remain visible. The separate 128-pane resize follow-up has 384 rows, exact original-process concatenation, 96 samples/12 warmups per ABBA process, matching seven-field work signatures, and 192 samples per condition. Its nearest-rank pooled p50 is **1,550,583 -> 1,523,542 ns**, a **1.743924704%** reduction. Per-process p50s are baseline 1.540542/1.553375 ms and candidate 1.524291/1.514000 ms. Both follow-up summaries recompute exactly. The document correctly treats this as a separate check that did not repeat the small regression, not as replacement of formal cells or a stable resize-speedup claim.

The disclosed tails match the raw summaries: group-1 1,024 resize max 17.158375 -> 21.319333 ms; group-2 1,024 idle max 8.898916 -> 13.040541 ms; group-2 16 resize p95 0.203292 -> 0.221584 ms and max 0.214541 -> 0.274916 ms. No universal tail improvement is supported or claimed.

The candidate/ImGui reference has 864 valid raw rows; all 18 p50/p95/max summaries recompute exactly and original process outputs concatenate exactly. The 1,024-pane reference p50 ratios are idle **6.177675546x**, click **7.535345970x**, resize **14.746318787x**, correctly rounded to 6.18x/7.54x/14.75x. These use the same final candidate but are a separate run and remain product/framework ratios, without subtracting headless times.

## Complete headless summaries and in-flight scope

The formal headless table contains **240 rows**, eight processes with 30 unique complete rows each: three pane counts × five scenarios × update/settlement. Each process portion exactly matches its original stdout. Headers, keys, two tabs, inactive DHAT, `time_ns`, 48 samples/six warmups, phase/end state and output counts all match the requested workload. Each p50 <= p95 <= max and all formal times are positive.

The four 1,024-pane selection-update process p50s, in execution order within each condition, are:

- Baseline: **9.746708, 9.561166, 9.614917, 9.706167 ms**.
- Candidate: **1.793458, 1.787375, 1.803500, 1.795750 ms**.

All five headless update rows in the document match the actual per-process p50s. Idle settlement range 0.512458–0.519833 -> 0.187084–0.192250 ms and selection settlement 1.006333–1.034792 -> 0.246959–0.250917 ms round as reported. The retained measurement-change update maximum increase is actual: 12.200375 -> 12.203708 ms across captured processes.

These files contain **process percentile summaries**, not raw per-update values. Their internal 48-sample percentiles cannot be independently recomputed from this archive. The document correctly shows process medians instead of treating two summaries as 96 raw samples, manufacturing a pooled p95, adding update/settlement p50s into an end-to-end percentile or subtracting them from product frames.

Formal timing and allocation commands explicitly use **`--in-flight 1`**, overriding the source default of three. All rows agree: ordinary update 0 -> 1 pending output, in-flight update 1 -> 2, settlement 1/2 -> 0 respectively. The source really performs one untimed prefill, complete owning program recording, confirmation and commit, then a timed update; later reverse Presented reports and a host-frame commit settle the pending outputs.

The in-flight scenario also alternates bounds 16,384/16,448 and intrinsic 64/72. The source trace correctly distinguishes it from fixed-measurement selection. Current-candidate/confirmed-fallback eligibility prevents its selection rebind; complete answer equality would also reject the newest unconfirmed candidate's changed inputs. An older retained owner can match the returning profile with this odd prefill count, but that is a future bounded source-choice opportunity, not permission to use stale output authority. The 8.295125–8.414500 ms process-median range is not isolated ledger cost, a proved floor or an attribution obtained by subtraction.

## Allocation and retained memory

Each allocation file has 180 unique complete rows: 30 inactive-DHAT time rows plus 150 active-DHAT rows for five metrics. All metric/instrumentation keys, six samples/two warmups, phases and exact pending-output counts match. These are separately summarized observations; the underlying per-sample heap boundary values are not emitted. The result document's five update rows match the actual active-DHAT p50 columns exactly.

For 1,024-pane selection update:

| Active-DHAT metric | Baseline p50 | Candidate p50 | Recomputed reduction |
| --- | ---: | ---: | ---: |
| Allocation events | 68,729 | 15,492 | 77.459296658% |
| Requested bytes | 46,398,268 | 19,121,728 | 58.787840960% |
| End-of-update live tracked bytes | 30,661,376 | 22,899,080 | 25.316202378% |

The leading 77.46%/58.79%/25.32% claims are correct. Event/request metrics are interval deltas; live-before/live-after metrics are absolute tracked heap boundaries. No independent live medians were subtracted and mislabeled as a median per-sample delta.

Selection case-prefix peak is 32,019,424 -> 23,701,960 bytes; in-flight peak is 53,382,664 -> 50,572,984 bytes. For these rows p50/p95/max happen to agree, but the metric is still the cumulative case-prefix high water including setup, warmup and earlier samples, not an isolated update-window peak. Settled selection live bytes are 14,615,880 -> 13,888,248 and settled in-flight bytes 14,616,256 -> 13,888,624. Settlement event/request medians stay equal except drag, which adds exactly three events/288 bytes, as disclosed.

This is complete headless active-DHAT evidence including owning recorder/output lifetimes. It is not RSS, egui normal-System-allocation memory, GPU memory, an ImGui ratio or a fixed per-scene saving. The documented representation additions and remaining transaction/root-fingerprint/selection-map copies correctly limit the memory claim.

## Fresh CPU attribution

Both profile receipts identify the final frozen candidate hash, succeed for the product and sampler, and occur after qualification/allocation/resize processes. The 1,800 idle and 480 click workload CSV rows are complete with the expected pane/case/frame identities. Their timings are instrumented observations, not formal qualification data.

I hashed both raw captures and independently parsed their call trees. Each graph contains exactly one thread, the main thread: **5,851** samples for idle and **5,871** for click. Every stored category's selected root list, root-count sum and three-decimal percentage reproduces exactly. The patterns agree with the existing architecture-review parser. The result document's ten displayed categories match the attribution JSON.

The category union is disjoint only within that category. Different categories overlap. Sampling includes initialization, warmup, untimed preparation/hover, validation and disposal, so percentages cannot be added into frame-stage budgets or converted into predicted savings. Inlining also prevents a zero-observed-symbol count from proving zero cost.

The output-audit category includes actual `product_cost::Panes`/`PaneView::ui` descendants, `paint_pane`, label layout and widget rendering. For example the idle graph contains a 254-sample application-pane UI subtree below `PaintAudit::check_append`; click contains corresponding real application UI descendants. The reported 17.006%/14.648% inclusive audit trees are therefore not a removable auditing budget. The document states this correctly. Constructor bypass is supported separately by source ownership, the complete conservative-plan oracle and inspected successful focused tests, not by a profiler-derived assertion of zero work.

## Serial order and successful checks

All **37** outer check/build/measurement/profile intervals have positive duration and do not overlap. The four reference processes and two builds nested within the product-build/reference command are also sequential. Actual UTC ordering is:

| Stage | UTC interval |
| --- | --- |
| Eleven final checks | 11:55:12.164783–12:01:58.494509 |
| Product build/reference | 12:02:40.007884–12:04:23.408544 |
| Formal product ABBA/BAAB | 12:04:23.425798–12:04:40.484469 |
| Headless build | 12:04:40.527891–12:05:53.566919 |
| Formal headless ABBA/BAAB | 12:05:53.578184–12:06:35.170078 |
| Allocation baseline/candidate | 12:06:35.171075–12:07:41.398998 |
| Resize follow-up ABBA | 12:09:49.983533–12:09:50.738139 |
| Idle CPU capture process | 12:10:30.824481–12:10:44.441837 |
| Click CPU capture process | 12:10:44.446358–12:11:00.860105 |

There is no build/test/active allocation profiler overlapping a formal timer in these receipts. This establishes runner serialization, not universal absence of unrelated desktop load or power-state variation. ABBA/BAAB reduces order drift; two groups on one host are not randomized multi-host confidence intervals. The document preserves those limits.

The log summaries establish 14/14 focused, 2,186/2,186 workspace, 292/292 default host, 335/335 serde host, 556/556 native and 10/10 doctests, plus successful workspace/native correctness-and-suspicious Clippy, formatting, dependency-boundary and diff checks. Focused tests are included in the workspace total, not additional unique coverage. The opt-in layout probe and native wall-time test remain skipped. Test-only and existing native/stale/unpresented/lifecycle coverage are described without claiming exhaustive combinations.

## Archive and final document

At archive inspection, the directory contained 49 files totaling 787,344 bytes before this final report is copied. The 47 ordinary data/report/log files compared byte-for-byte with their /tmp originals after decompression. The remaining source receipt is intentionally normalized: its current commit, binary hash and verified-file count match the original, and its relative reference resolves to a candidate environment exactly equal to the original embedded environment. The README is the archive index. This normalization preserves provenance without duplicating the large source table.

All relative links in the main result document and final README resolve, including the product comparison script and existing attribution parser. The index correctly distinguishes formal raw product data, summary-only headless data, inactive allocation-run time rows, standalone resize follow-up and instrumented profile observations. Historical pilots and failed implementation checks remain in /tmp and are not promoted into the final successful freeze. Runtime binaries remain outside the repository.

No arithmetic, provenance, workload-preservation, serialization or memory-scope blocker remains in the inspected final evidence. The plan/goal gate is met for the declared workloads. This approval does not imply a GPU/native-window result, latency guarantee, RSS improvement, constant-time mutation, eliminated full measurements or a universal theoretical performance floor.
