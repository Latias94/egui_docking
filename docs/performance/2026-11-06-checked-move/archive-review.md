# Checked-move archive: independent sealing review

Reviewed at 2026-11-06T15:25:54.721613+00:00. Archive: `docs/performance/2026-11-06-checked-move/`. Original evidence: `/tmp/dockspace-checked-move-20261106-a2bcabc/`.

**Verdict: no blocking archive finding. Compressed contents, original-byte receipts, declared counts, links, source/binary identity and retained negative evidence are consistent.** Root may finish the checksum manifest and precise staging/commit. No further optimization, runtime change or measurement is requested by this review.

This reviewer read existing files and decompressed gzip/tar streams in memory; no archive was extracted onto disk. No repository file edit, Cargo, compiler, test, measurement binary, benchmark, profiler, network, Git write or nested agent was performed. The only write is this temporary report. The newly mentioned browser guidance has no bearing on this local sealing review.

## Archive inventory and original-byte equality

At the review snapshot, before this report is optionally copied and before final `SHA256SUMS`, there are **69 stored files / 5,576,579 bytes**. All dataset and log compression was inspected:

| Dataset | Raw rows | Group summary rows | Sample-zero full vectors | Process-output members |
| --- | ---: | ---: | ---: | ---: |
| `frozen-comparison/` | 8,064 | 84 | 168 | 24 |
| `candidate-reference/` | 8,064 | 84 | 168 | 24 |
| `frozen-comparison-v2/` | 8,064 | 84 | 168 | 24 |
| `candidate-reference-v2/` | 8,064 | 84 | 168 | 24 |
| `resize-followup/` | 4,608 | 24 | 48 | 24 |
| **Total** | **36,864** | **360** | **720** | **120** |

These are exactly the README's five dataset rows. The two final datasets also retain **84 pooled summary rows in total**. Every `raw.csv.gz` decompresses to the original raw CSV bytes; each summary/environment/pooled file equals the original file. Decompressed raw/summary SHA-256 values equal the original-byte hashes inside their dataset environment, rather than incorrectly comparing those receipts against compressed storage bytes.

I checked **11 tar archives / 847 members**: 720 state files, 120 per-process CSV/stdout/stderr files and seven v1 source copies. Every member equals its exact original bytes. Each dataset's state and process-output archive contains the exact original filename set, with no duplicates, omitted process or renamed state path. Original process-directory names are preserved inside state archives. Empty stderr files are retained.

Every tar member is a relative regular file, with no parent traversal, symlink or hardlink. Member names are sorted and unique. Headers are deterministic: mtime/uid/gid zero, empty owner/group names, mode 0644 and no extra PAX headers. All 11 tar gzip streams and the **25 standalone gzip files** have zero timestamp and no optional filename flag. No binary executable is present in the committed archive.

Beyond tar members, **56 archived files** match a corresponding original file byte-for-byte, after decompression where needed. `README.md` and `checks.json` are the two intentionally generated archive-only files; their content was checked separately. `baseline-build-environment.json` equals the previously audited baseline environment, and the archived final/first/implementation review reports equal their current temporary originals, including the completed resize addendum.

`v1-source-copies.tar.gz` has exactly `canonical.rs`, `checked_move.rs`, `drop_resolver.rs`, `graph.rs`, `operation.rs`, `validation.rs` and `workspace.rs`. Every member equals its `v1-sources/` copy and its exact source entry in the first candidate environment. The original v1 graph implementation is preserved. The six-file `reviewed-runtime.json` is explicitly identified as the earlier v1 receipt, so it is not confused with the final candidate's full source map.

## Source, executable and receipt connections

All **325 final candidate source hashes** still match the current repository inputs. Each before/after environment points to the same frozen baseline executable hash; its archived `baseline.json` agrees with `baseline-build-environment.json`. Current local frozen baseline/final executable bytes still match every recorded before/after/control binary hash. These copies are read only and remain in the documented temporary locations. Prior implementation/evidence review snapshot hashes and final environments preserve each iteration's identity rather than treating the base commit label as its modified-source receipt.

All **49 protected user files** retain their starting hashes. The archive contains the user's existing failure evidence and reviews only where explicitly generated as experiment reports; the user's unrelated CI/review files remain untouched. This read-only audit does not stage any file or change Git state.

The README's actual Markdown links to the result and plan resolve. The result document's links to its plan, prior matched-kernel result and archive also resolve: **five local links checked**. Every independent report listed by `checks.json` exists, as do all check-log references. Local reproduction commands are explicitly described as experiment evidence; recorded temporary executable paths are not presented as a new portable project CLI.

## Verification receipts and retained failures

`checks.json` has nine successful current check records. Its six final auxiliary execution receipts are exactly the corresponding objects in the original `final-checks.json`, including commands, return codes and intervals. All archived build/check logs decompress to the original bytes.

Existing logs independently substantiate:

- Final ordinary all-feature core: **2,023 run / 2,023 passed / 3 opt-in probes skipped**.
- Public-facade integration: **4 run / 4 passed / 0 skipped**.
- Documentation: **9 passed / 0 failed**.
- Ordinary enabled-counter v2 probe: **1 passed**, with all 21 work cells retained.
- Clippy correctness/suspicious, formatting, Python compilation, dependency boundary and diff checks: receipt return codes 0. The Clippy log contains style warnings, and both results/checks explicitly avoid a warning-free claim.

The original default-core fail-fast log is retained verbatim: **828/1,952 run, 827 passed, one failed, three skipped**, including the obsolete work-counter assertion. Subsequent complete 2,019-, 2,021- and 2,023-test passing logs remain distinct. The failure is not relabeled as a passing current suite or removed from the evidence.

No check was rerun by this reviewer. These inspected logs close the prior review's “root verification still pending” temporal note; earlier reports correctly describe their own snapshots, and the final checks receipt is the authority for completed root execution.

## Failed first gate, resize limitations and document wording

The archive README explicitly labels the first before/after dataset as **four-view gate not met**. Both first datasets, their raw samples/tails, process output, source copies and binary identities remain separate from final results. The result document still states the first four-view reductions **19.14% / 18.79%** were below the plan's 20% criterion. It does not silently substitute v2 for those rows.

The final formal negative resize cells and separate control are retained, with original settings/sample counts and independent receipts. `checks.json` explicitly states that small one-view median slowing and mixed tails remain unexplained and makes no zero-regression/universal-stability claim. The result document preserves the repeated formal 128-pane/four-view 203/225-us maxima and the residual one-view follow-up shift, does not pool the control into formal statistics, and does not assert that noise or a specific cache/codegen mechanism was proven. This is consistent with the completed evidence review.

The final result document at this sealing snapshot has SHA-256 `0f4b0c415c0e999fc8b743c29c6b1f9e72c4fef7c70bc4b51e292bbdca59d575`. Its added qualification paragraph matches the now-complete logs and narrowly describes four public-facade tests, rather than claiming an unrun full workspace suite. No new numerical or wording blocker was found.

## Checksums and final sealing boundary

`SHA256SUMS` was intentionally absent at this snapshot, as root announced. It must hash the **stored bytes** after all final archive copies, including any copy of this report; original environment hashes continue to refer to decompressed raw/summary bytes. The absent not-yet-generated manifest is a known final packaging step, not a detected integrity failure. Do not treat the 69-file/byte total above as the final count after adding this report and manifest.

Snapshot identities:

| Archived file | SHA-256 |
| --- | --- |
| `README.md` | `eb917be1a8b89967e4ab785d137e6745325acb52d5ef1ab492a1a60b18d7ccc5` |
| `checks.json` | `89bc76eabe12f9e3c6b964dd2c17155e6a37f20f4b83ac3aa58c8d00d8e61771` |
| `final-evidence-review.md` | `5629fdc80c513791e141e79916d92adf1127ec6bd9be0d2b4f6ae4720c5daaa0` |
| `first-evidence-review.md` | `e4837d4f05837b5f96b5755d7f404e62d75989c066060192a4c064548054d99e` |
| `v2-implementation-review.md` | `3aaa41b1e0c64278e7cdef00114ced91b1952f53c204a35e922755d421143f11` |
| `baseline-build-environment.json` | `21b74f8422548f5304cd6bf43adace734f9011a5735ad02f3bfbfaa8d2a64777` |
| `baseline.json` | `af87c1973d2f8b881393efe3e5e107cc74d91cfc8e91d96858b926f572946cc9` |
| `v1-source-copies.tar.gz` | `dcc86797f7c990fdfe25a45d1c03d5bf680389710c319df05e37b5aabddd23f1` |

Final finding count: **0 blocking or required content fixes**. Remaining root-owned step: generate the final stored-byte checksum manifest after the last copy, then precisely stage the authorized runtime/tests/plan/performance evidence while excluding the 49 preserved user files. No additional sampling or optimization is needed to close this archive review.
