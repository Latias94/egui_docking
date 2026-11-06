# Independent evidence review: candidate-copy checkpoint

Date: 2026-11-07. **The evidence supports retaining this slice as an intermediate improvement. It does not satisfy the complete p50 <= 2x ImGui goal.** No blocking qualification, source-identity, sample-selection, state-equivalence or archive-integrity defect was found. Two nonblocking documentation/provenance clarifications are recorded below.

This review reads the final qualification and measurement evidence rather than repeating common's source review. No repository edit, Cargo/compiler/test execution, new timing, benchmark, Git write or sub-agent was used. All numerical checks below recompute existing data. This report is the only new artifact written by this review.

Root subsequently committed the copy checkpoint and began the separate cold/initial-issuance work. The authoritative copy source is therefore immutable commit **`fbc865945ed8bb8c3acc1ab10190bad97fab7dc6`**, its frozen source receipt and archived patch/added files. Later working-tree differences are not copy drift.

## Source identity, actual baseline and qualification

I independently materialized the two commits through read-only Git archive into memory and compared every recorded path by SHA-256. The 323 copy inputs equal fbc8659 exactly; the 321 prepared-report inputs equal **`93f2c570af48ff2c81e1b7f61d6977f17522016a`** exactly. The receipts cover every Rust file under crates/dockspace, the root Cargo.toml/Cargo.lock, the core manifest and the C++ docking probe. No Rust file in that defined core-crate scope is omitted. This is a complete core-crate receipt, not a claim to hash every host/workspace/dependency/toolchain file.

| Frozen identity | SHA-256 |
| --- | --- |
| Copy source-receipt.json | `0b5f815b852e5a1232e3630635e477cea3106cc9ab5061ce2f3ea51b408b9744` |
| Prepared-report source-receipt.json | `26b5162117c2efd782c64b47ef0ae11b567f4a10499affe6c96ef01b135feeb5` |
| Copy executable | `edea256dcd06cbef3887d49f702b927636be2adb5a1d2df08883b38d800fcc76` |
| Actual 93f2c57 executable | `7d9f49c5dd06005d9cd64028e9f86857c9009411bfbebad717d87a84e5fe7b2a` |
| Pinned ImGui executable | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

Both comparison environment files contain exactly the frozen copy's 323-source map. All three currently retained executable files match the recorded hashes; the run receipts also record unchanged binaries at completion. Against-baseline actually selects the prepared-report-frozen executable above, not ea4161e or a freshly rebuilt control. Against-imgui selects the unchanged executable for pin `d88f238d1d363ef387382b2e29ad12cf96f826ac`.

The archived runtime.patch also equals the immutable 93f2c57..fbc8659 tracked-core diff exactly, excluding the two separately archived added files; both added files equal their committed bytes. Runtime patch SHA-256 is `1e5722338f6d8f77a08e91b5573683681834142f27f0497ad4c4147417f8f65d`. This closes the source snapshot against the actual selected control, rather than relying only on path names or a partial changed-file receipt.

The completed normal-counter manifests each contain 322 inputs, precisely the corresponding frozen core-source subset excluding the C++ file. Every hash matches. The final nine-gate and two-host-gate receipts list the actual completed commands and zero return codes; their filenames alone were not used as evidence. Logs contain the completed summaries:

| Executed correctness gate | Observed result |
| --- | --- |
| Focused copy/local/owner/transaction/layout/recovery | 124 passed |
| Production-profile all-feature core | 2,058 passed; 3 opt-in probes skipped |
| Workspace all features/all targets | 2,234 passed; 3 opt-in probes skipped |
| No-default-feature public integration targets | 4 passed |
| Independent host, default features | 292 passed |
| Independent host, persistence feature | 335 passed |
| Formatting, correctness/suspicious Clippy, strict docs, doctests, dependency checks | Completed successfully |
| Separate ordinary work probe | 1 passed; all 21 default cells written |

The stricter late-second-command failure assertion is present in the committed source and passing test logs. The prior source review's temporary-only status and earlier partial-gate checkpoint are historical sections, superseded by these completed receipts. No failed or unfinished ordinary gate is being relabeled as a pass.

Timing uses the production-profile cfg(test) ignored probe, compiled with `DOCKSPACE_BENCH_NO_WORK_COUNTERS=1`; the build command uses default core features. Normal qualification includes the explicit all-feature and no-default-feature commands above. Source identity does not imply the same feature configuration or executable for those different gates.

## Ledger bypass is narrow and the work CSV remains honest

`structural_work::update` returns before invoking its closure when the compile-time environment variable is present. The O(N) WorkspaceCloneVolume capture for transaction/data candidates is inside that closure. Node/MRU store/payload hooks use the same entry; evaluated scalar handle counts/lengths do not introduce an extra graph scan. The bypass does not skip validation, canonicalization, command authorization, state export or real copying.

The formal timing test requires the compile-time variable to be present; the ordinary work test requires it absent. Both actual probe runs succeeded under their respective build/run receipts. The work CSV also contains nonzero physical clone events and full logical inventories on every move, ruling out accidentally using the disabled counter build for attribution. This review does not claim every unrelated cfg(test) mechanism is removed from the formal binary.

I checked all 21 rows, their exact case roster and all 20 columns. Resize has zero forks/payload copies; every move has one transaction candidate, one NodeStore fork and one MRU store fork. Logical nodes remain `2 * panes - views`, logical tab inventory remains `2 * panes`, live Node handles equal that complete node inventory, and live MRU handles equal panes. At 1,024 panes that means 2,047/2,044 Node handles and 1,024 MRU handles, not zero copying.

Center/cross-view moves record two Node payload clones/four tab items and two MRU payload clones/four history items. Edge records two Node payload clones/two tab items/two child IDs/two weights and one MRU payload clone/two history items. Every primary sample has zero root-fingerprint builds/visits and zero item-multiset scans. These are the actual counter fields, not inferred payload sizes or a substitute for allocation/peak-memory evidence.

The work snapshot covers the existing complete measure call after fixture setup and before validation/release. Its schema addition does not change the formal timing CSV or its timer. Capacity/holes, fresh vectors, ordinary domain snapshots and authority/placement copies retain their documented exclusions.

## Fixed matrix, raw rows and complete vectors

Both campaigns use exactly 16/128/1,024 total panes, one/four logical views and the applicable resize/center/edge/cross-view roster. Cross-view is absent only for one view, leaving 21 distinct cells. Each process records exactly 48 samples per cell after eight warmups: 1,008 rows per process. Each group pools two runs per condition, so a summary percentile has **96 observations**, not 48. The two groups use serial ABBA then BAAB, with all recorded run intervals ordered and every return code zero.

I verified each run CSV's complete Cartesian sample set, product, graph/item/root counts, alternating resize widths, nonnegative phases and exact phase sum to operation_owned_ns. Concatenating the eight run CSVs reproduces each campaign's raw.csv exactly: 8,064 rows per campaign and 16,128 total. No sample, negative cell, warmup count or operation roster was silently filtered.

Every one of the 168 summary rows was independently reconstructed from raw values using the documented nearest-rank p50/p95 and maximum, for all five timing fields. All 84 public comparison rows then reproduce the summaries' values, ratios and reduction percentages.

All **336 complete vector files** were compared byte for byte by cell across both campaigns. Independent little-endian FNV recomputation of every vector matches the corresponding first-sample digest. All timed-row digests also agree per cell and width, including the alternate resize width. The archive stores one full vector at sample zero per cell/process, not a full dump for every timed observation. The Rust probe checks every actual complete owned Vec against its expected Vec before emitting; C++ uses throwing `require`, still active under NDEBUG, to compare live state and every exported word on every sample. Thus full-state correctness is not inferred solely from a digest or item count.

Setup, postvalidation/digest/IO and rich-result destruction retain the original outside-timer scope; actual request capture, checked apply/replaced Workspace disposal, all-root projection and fresh owned export retain their inside-timer scope. No new ImGui rendered-click or native-viewport equivalence is claimed.

## Gains, regressions and goal accounting

All 30 move group/cells improve p50 against prepared-report, by 6.77–30.77%. Every 1,024-pane move improves in both groups, by **25.39–29.55%**. The ten negative p50 cells are all resize, including all four 1,024-pane resize group/cells. Their 1,024-pane regressions are **1.55–5.47%**. The candidate is therefore a tradeoff that improves movement and makes most resize medians worse, rather than a uniform win.

| Comparison to 93f2c57 | Negative group/cells | Worst retained regression |
| --- | ---: | ---: |
| Complete p50 | 10/42 | 7.9137% |
| Complete p95 | 11/42 | 123.9238% |
| Complete maximum | 15/42 | 979.0298% |

The worst p50 regression is group 2, 128 panes/four views/resize. The worst p95 and maximum are group 1, 16 panes/four views/resize: p95 3,833 -> 8,583 ns and maximum 3,958 -> 42,708 ns. All other negative p95/max cells remain in the unfiltered comparisons CSV, and their individual raw samples remain in the archive. Tail gains elsewhere do not justify a general tail-improvement claim.

| Panes | Candidate complete p50 / ImGui range | <= 2x group/cells |
| --- | ---: | ---: |
| 16 | 3.7024–7.2000 | 0/14 |
| 128 | 1.7415–3.1906 | 2/14 |
| 1,024 | 1.2908–2.0367 | 11/14 |

The recomputed total is **13/42**, versus the previous prepared-report table's 8/42. All 42 p50 values remain slower than ImGui itself; 35 p95 and 27 maximum comparisons are also slower. Pooling independent groups would hide information, so they remain separate.

Observed large-move mutation medians are 18.083–19.333 microseconds in the Rust-control campaign and 18.000–19.041 in the ImGui campaign. They are lower than the control's separately observed 77.625–81.667. This is consistent with the work counters and complete-operation gains, but subtracting phase medians is not a per-sample attribution. Neither the remaining mutation time nor Arc sharing establishes a lower bound on further structural optimization.

The production recovery test's actual 0.042-second nextest line is retained. Comparing it to the earlier coarse 0.026-second observation is appropriately described as a negative observation, not a formal isolated cold-path timing conclusion.

## Archive integrity and two clarifications

Archive SHA-256: **`7fb25f40bb7a8896860455acb840466ac3115e88752e4ee7495927fb9a73d07e`**. Size: 366,712 bytes; regular input files: 428; archive regular members including manifest.json: 429.

I independently read every member, checked unique safe relative names, exact byte count and SHA against manifest.json, then compared its complete bytes with the original input under /tmp. Every comparison succeeds. The archive contains exactly 16,128 aggregate raw rows and 336 state vectors as reported. All six public SHA256SUMS entries match. Public work.csv and review.md equal their archived/original counterparts. Qualification logs, complete source manifests, exact run/build commands, runtime patch, added source files and original proposal/review inputs are preserved.

Two nonblocking clarifications improve the written evidence without changing the retain decision:

1. **Historical baseline.json:** this archived generic file still names ea4161e. It is not the executable used by against-baseline. The actual run metadata, independently verified executable hash and prepared-report source receipt bind the control to 93f2c57. Label this file as historical provenance if citing it; do not present it as this campaign's selected control. The prepared-report source receipt is in its separate checkpoint, rather than embedded in this copy archive; the exact commit and binary binding above close that check for this review.
2. **Phase range wording:** the prose's approximate “layout 110–113 microseconds” does not span both copy campaigns. Large-move layout p50 is 110.208–115.750 microseconds against the Rust control and 109.834–113.875 against ImGui. “About 110–116 microseconds across the two campaigns,” or explicitly naming the campaign, matches the preserved data more closely. This does not affect the correct complete-operation ratios or regression counts.

Retaining the slice is supported by repeated complete-operation move gains, unchanged full-state results and completed correctness qualification, with the resize and tail costs explicitly accepted and preserved. It is an intermediate result under the continuing goal; it is not evidence that all cells meet 2x, that the remaining fork/layout costs are irreducible, or that allocation/peak-memory savings have been measured.
