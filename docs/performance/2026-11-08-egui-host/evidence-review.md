# Final egui host exercise evidence review

**No blocking evidence or statistical error found.** The report accurately describes one successful, software-rendered real-window journey, with separate headless semantic checks and failed earlier attempts. It does not establish hardware-GPU performance, physical OS input, a unique cause for earlier stalls, or a new ImGui-kernel result.

I statically reviewed `docs/performance/2026-11-08-egui-host-exercise.md` and its small evidence directory. No build, test, GUI, timing, profiling or network action was executed.

## Independent arithmetic and sample checks

`frames.csv` contains exactly 384 rows: the six declared scenarios in order, each with 64 rows and sample indices 0 through 63. Every row has positive actual vertex/index counts.

I independently recomputed all four timing fields' p50, p95 and maximum for each scenario, as well as vertex/index ranges. All match `summary.json`. P50 uses the mean of the middle two observations; p95 uses nearest rank. Every value displayed in the report's table matches the independently recomputed value rounded to three decimal milliseconds, including the UI-to-receipt p95 and maximum columns.

| Quantity across all 384 rows | Recomputed value |
| --- | ---: |
| Sum of `paint_present_ns` | 38,718,434,931 ns |
| Sum of `ui_to_present_ns` | 38,865,380,479 ns |
| Ratio of those sums | 99.6219114642% |

The reported 99.62% is correct. It is a ratio of directly nested elapsed call intervals across the same rows, not a sum of medians or an exclusive GPU/rasterization share. The report states the acquire/upload/encoding/submit/present and blocking limitations appropriately. No attribution to docking alone follows.

The successful command receipt spans 47.245601 seconds, consistent with the report's 47.25 seconds. Its log records all six phases complete, 384 UI samples and zero startup dropped outputs. Unrecorded setup/warmup/press/release/validation frames are correctly distinguished from the CSV rows.

## Source and artifact binding

All seven source/report hashes and 22 artifact hashes listed in `checksums.json` match the files read during this review. `environment.json`'s CSV hash also matches the raw file.

The successful run's manifest, lockfile and both Rust-source hashes match the current repository bytes. Both Rust hashes also match the final code-review closure:

- Demo: `58033b120d976d04f7753eceb5a94e05a31d5fc09b85287b9f8dd807327ccfb3`.
- Exercise: `0c84a6c7fa9f4a62ce360a86255c35cfcee844d6e43e2d225fb84ef37f2107f2`.

The README has intentionally advanced since that source closure to add documentation; its current checksum is independently bound in the evidence manifest. This is not runtime drift.

The build receipt reports a successful Linux production build. The successful environment binds binary SHA-256 `6a9bda6032f4fbf4652120fce3f5b7d4581aea5f9200d355e0e089c62fda5490`. This review verifies the recorded binding and source files, not a new inspection or execution of that container binary.

## Claims and check status

The actual run is correctly labeled Linux aarch64/Xvfb, GL, llvmpipe CPU adapter, one CPU quota, 4GiB, `LP_NUM_THREADS=1`, 1280x820, scale 1 and AutoVsync. Real renderer submission and the backend present call are exercised; this is not M4 timing, GPU execution duration, scanout, or physical mouse input.

The default-worker debug and production failures remain preserved rather than replaced by the successful result. The report correctly avoids a numerical before/after gain or sole-cause claim: no complete default-worker CSV or shader-cache isolation exists.

The headless journey's test-owned Presented acknowledgement and zero timings are explicitly identified as semantic evidence. The verification records and logs agree:

- Formatting passed.
- Official nextest suite: 25 tests passed, including the semantic journey.
- Clippy correctness/suspicious checks passed under the recorded arguments; this is not a claim that every warning was denied.
- Linux headless journey: one test passed.
- Doctest attempt returned 101 with “no library targets found.” It remains preserved and is marked not applicable, not passed.

The CI wording describes the changed gate definition, not an assertion that an exact-head remote CI run has already passed.

The final packaging-only change stores the Linux headless log as deterministic gzip. I independently decompressed `linux-journey-test.log.gz`; the resulting SHA-256 is `2986499499130ed6e0bc37e2e79e5aed38420bf158ef35ad5ff37b816d880870`, exactly the original log binding. All 22 artifact checksums match after that packaging change. The test evidence and result are unchanged.

## One nonblocking provenance clarification

The archived macOS command/log directly establishes Apple M4/Metal, `surface occluded`, no accepted workload CSV and unsuccessful Setup. Those two files do not themselves contain the separately observed `CGSessionScreenIsLocked=1` query.

The report's locked-session explanation is consistent with Root's recorded observation, but Occluded alone does not prove a lock. If stronger self-contained provenance is desired, qualify that sentence as an operator-observed locked session or append the already captured lock observation; no rerun or new platform diagnostic is required. This does not block the report's central conclusion that no M4 timing is available.

## Reviewed document bindings

| Artifact | SHA-256 |
| --- | --- |
| Final report | `035471e489a1285d84601280887c53089ec21db009f082a7e724a63348f7f98e` |
| `frames.csv` | `1fc2e1051f511370241f22a950c8aa4c4efcbd1cbca30319b27ad7f4e0e0b347` |
| `summary.json` | `f2ccaaf7813ebd74c41ea25be0e9e0b7d8264e65ef119831a63c0bfc99ff69f0` |
| `verification.json` | `55705a5e3f834e88d99977c6140f104143b2b753d8df893742fef14594f96983` |
| `linux-journey-test.log.gz` | `8a4231610c11223ecdafbf8abf0b553b006a90e809bc74482673d9a8643dfbbe` |
| Final `checksums.json` after gzip packaging | `3b1d6bd26725439c5f98e71eb08516d3218452a6b8f4fb1305f72e6798bdc042` |

Only this review was written. No repository or evidence file was modified.

## Closure: macOS observation provenance clarified

The report now states in its opening that the Apple M4/Metal attempt produced no accepted timing samples. Its failed-attempt section explicitly separates the archived `surface occluded` renderer evidence from the independently observed locked-session state. This closes the sole nonblocking provenance clarification above.

This was a narrow wording check, not a repeated source or evidence audit. The raw CSV and summary hashes remain unchanged. The previous report and checksum bindings above are retained as historical review bindings; the clarified versions read for this closure are:

| Artifact | SHA-256 |
| --- | --- |
| Clarified final report | `96d6393a619c6769a131fe00961a7883da37935f45f4b41fffb59f8ea4b46225` |
| `checksums.json` after clarification, before adding this review | `6b5abc6105abe4da43109de12cd3544b98da2b19f17d4bb6a594ac26bd812bc2` |

The checksum manifest's report entry matches the clarified report. Adding this review to the evidence directory will legitimately create a later checksum manifest; this closure does not attempt a circular binding to that future manifest.
