# Live Tabs facts evidence review

No evidence blocker found. Rejecting the exact Live Tabs-only candidate and restoring control is supported by independent full-data recomputation: affected center/cross p50 improves in **4/18**, below the predeclared 12/18; small affected p50 improves in **1/6**, below 4/6. Whole complete p50 is **10 improvements / 30 regressions / 2 ties**. The candidate does not qualify for confirmation or the remaining ordinary gates under the declared early-screen protocol.

I audited the saved archive, manifests, source pairs, receipts, scripts, check logs, complete raw data and vectors. I did not execute archived scripts, Cargo, tests, binaries, benchmarks, profiling, timing or network operations, change repository/evidence files, or read another stop opinion. This report is an evidence decision for one rejected experiment, not a general optimization-exhaustion verdict.

**Exact binding.** Evidence is `docs/performance/2026-11-07-live-tabs-facts/`, with the corresponding temporary `live-tabs-facts-{frozen,verified}` directories under `/tmp/dockspace-imgui-twofold-20261107-ea4161e`.

| Artifact | Independently verified SHA-256 |
| --- | --- |
| `evidence.tar.xz` | `9f4153a448519da2b82b98f05a86c06eb9223b55d9a210153b63ec205308836f` |
| Internal `manifest.json` | `78d6203362eaa4bfca95b89a9f2202299353b59d2986af07b26b767fd3eb50ef` |
| `archive-check.json` | `e339acda44e31319d7c774e57cc259e8acfafd08b29873c6cc0985f00ae84d76` |
| `comparisons.csv` | `9df90cffaf31b18ffcf8f18d9bee3f820681e2ad79b39bb6225ae055a8c7aefc` |
| `measurements.json` | `b9b1dbdfd1dc9bf2445e12627de86eeea2e83a6b1cc4fd04deb79a155abc117e` |
| `decision.json` | `f1569facd14248654efb8c590cd4c940fc15fe94ba15e3ca00fbbb4322a3434a` |
| Candidate source receipt | `760d2266efc79b44856ab84cb47bde0e35329d6c2f7725a8096401da3c8928ef` |
| Candidate patch | `57e65ea61be7f8cdb58681a9fb3d037dc0dc322b6d98e487862a1e933203fe56` |
| Predeclared rule | `914a969a44584762685d2f5d5f09ffe9f668f26c02e37b1c9d7b951ab13d283c` |
| Actual Rust control executable | `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b` |
| Candidate executable | `903b610a46bd8e7a1f4250b3251f9aebe2af14e6e600285062e0f2de33081636` |

The 340,852-byte archive contains exactly **426 regular inputs plus its manifest**, 427 unique regular members total. All member lengths/hashes match the manifest, all paths are safe relative paths, and corresponding temporary files still match archived bytes. Public `SHA256SUMS` covers every other public evidence file, including the decision, and all entries verify.

**Source and qualification scope.** Actual control is immutable `c7508eec3ac44138305797691244ae9c180d7680`, retained runtime `0e441c8`. I matched every one of its 324 formal hashes to Git blobs. The candidate map differs in exactly `operation.rs`, `workspace.rs` and `operation/tests/checked_move.rs`; exact archived control/candidate/measured snapshots and the frozen patch bind the previously reviewed implementation.

The public design/implementation reviews match their archived source-package copies, SHA-256 `4eae2d9c001cc51bce8020385b00f3f8ec19639c85a05e93e0c157582631bc21` and `0ab7a9f56d6dd1377913cfee112d49d6bfee7103a3ebd9c72069ada9d568eac0`. This candidate contains no Tabs-capacity helper, debug-prefix slice, numeric change or alternate target-authority framework.

The source delta leaves frozen `layout/reference.rs`, actual layout/solver, `drop_resolver.rs` instrumentation, the formal probe, Cargo files and pinned C++ comparison adapter unchanged. The probe remains SHA-256 `01d8438fc79095aacae935170aff3ed7eb8b5e497aa368c9966889b789adf2d3`, including its original 10,000 cap. Fresh input collection, all-root layout, complete owned results, full common export and timer endpoints are therefore unchanged.

Exactly three ordinary gates are recorded successful: formatting, **433 targeted tests** and four focused regressions. The added paired transaction-priority witness explicitly appears as PASS. A separate ordinary-counter work test passes. The normal-check source inventory has 323 entries, all matching the timing candidate; only the unchanged C++ adapter is outside that Rust-gate inventory. The successful optimized timing build uses the standard work-counter bypass and matches the frozen candidate executable hash.

Neither the receipts nor the decision claim all twelve ordinary gates passed. The predeclared rule precedes these checks and measurements and requires full qualification before retention, and one additive confirmation only for a promising initial result.

**Complete raw and vector audit.** Both campaigns use serial ABBA then BAAB, eight runs each, 48 measured samples/eight warmups per process/cell, yielding 96 observations per condition/group/cell. All 16 saved workload commands succeed; Rust stdout reports one passing probe, ImGui stdout equals its captured CSV, and stderr is empty. Executable hashes and result hashes match both environments.

I checked all **16,128 raw rows**, including every process/cell/sample, full inventory, resize widths, positive complete durations, nonnegative phases and exact telescoping time identities. Every aggregate row is reproduced by its per-run CSV. I independently recomputed all **2,520 summary p50/p95/max values** across the five phases using the unchanged nearest-rank convention, then all **84 complete-operation comparison rows** and every aggregate in `measurements.json`.

All **336 vector files** are present and parse as complete node/item/geometry word streams with the expected node/leaf/split/root counts and full item permutation. Their recomputed word digests match raw sample zero. All 16 versions of each of the 21 cells match byte-for-byte across the two campaigns; all 27 cell/width digest groups agree across the raw observations. The output-equivalence conclusion does not rely only on receipt booleans.

**Recomputed result and negatives.**

| Versus actual Rust control | Improve | Regress | Tie | Reduction range |
| --- | ---: | ---: | ---: | --- |
| Complete p50 | 10 | 30 | 2 | −10.4044% to +5.7683% |
| Complete p95 | 20 | 22 | 0 | −29.0248% to +32.5820% |
| Complete maximum | 19 | 22 | 1 | −389.4668% to +69.8964% |
| Affected center/cross p50 | 4 | 12 | 2 | 4/18 improvements; requirement 12 |
| Small affected p50 | 1 | 4 | 1 | 1/6 improvements; requirement 4 |

Small one-view center medians regress in both groups: 2,042 → 2,083 ns and 1,958 → 2,000 ns. Four-view cross improves in group one (2,167 → 2,042 ns) but regresses in group two (1,917 → 2,000 ns); selecting its favorable group would misrepresent the experiment.

Negative tails remain in the evidence. For example, group-two 128-pane/one-view resize maximum is 10,709 → 52,417 ns. The largest median regression is group-one 128-pane/one-view edge, 14,417 → 15,917 ns. These observations need no speculative compiler/cache/allocator explanation to establish threshold failure. Inclusive profiling observations or sums of phase medians are not used as causal shares.

The separate ImGui campaign has 30/42 conditions at most 2×: all 14 medium, all 14 large, and two small. Its p50 improve/regress/tie is 2/40/0, p95 5/36/1 and maximum 13/29/0. That target-inventory gate passes but cannot override failure relative to actual Rust control or establish a retained gain.

**Work and exact restoration.** The complete **21×20 work CSV** is byte-identical to control, SHA-256 `88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4`. I independently checked candidate/store/handle/payload counts, visual/MRU copies of 4 for center/cross, 2 for edge and 0 for resize, and six completed root bodies totaling 2,926 rows. These counters do not measure RootReference atomics or allocator events.

The decision's control, candidate hash, statistics, early-screen scope and reject/restore outcome match all underlying evidence. Independently hashing the current workspace confirms **all 324 formal inputs restored exactly**, including the complete added witness removal. No runtime/test fragment remains.

This closes the exact Live Tabs facts variant as rejected without confirmation or remaining nine gates. The audit finds no packaging, source, correctness-receipt or numerical-summary issue requiring a rerun. It does not prove that all authority deduplication is harmful or establish a performance lower bound.
