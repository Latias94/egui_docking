# Layout traversal: independent evidence and document review

Reviewed at 2026-11-06T16:18:08.508016+00:00. Inputs are the three frozen `layout-traversal/against-*` comparisons, their receipts/process files/dumps, the three preceding resize controls, the implementation plan/source review, and `docs/performance/2026-11-07-layout-traversal.md`. This review used file reads, hashes and arithmetic only. No build, Cargo, test, measurement executable, profiler, benchmark, Git write, network or nested agent was run. The only write is this report.

**Verdict: the final candidate satisfies the existing plan/goal p50 acceptance gate, with no blocking source/sample/hash/output-integrity finding. There is no evidence here requiring an additional runtime change under that gate. Two document corrections remain at the reviewed document hash: disclose the substantial 128-pane/four-view center tail in group 2 against checked move, and round the mutation share to 64.34%.** These are publication corrections, not grounds to discard samples or rerun measurements. Qualification is a median result, not zero regression or a latency upper bound.

Reviewed document SHA-256: `dbbddd879a7b7ae20298b5ec352a8187f3c599c78764e15f8aefc41d979387a3`. The exact outstanding locations are lines 94–102 (tail paragraph) and the phase-share sentence beginning “Same-execution phase sums” (currently 64.35%).

## Integrity, ownership and provenance

- Each comparison contains exactly 8,064 observations, eight successful processes, 84 group/condition/cell summary rows, 42 pooled rows and 168 sample-zero state files. There are exactly 21 cells, with 48 samples/eight warmups per process, 96 observations per group/condition/cell and 192 pooled observations per condition/cell. The ABBA and BAAB identities, sample ranges and per-process exact dump filenames have no gap, duplicate or extra entry.
- Independent arithmetic checked every row: all four measured phases are nonnegative, complete-operation time is positive, and request + mutation + layout + export equals the recorded direct total exactly. Original process CSVs reconstruct aggregate CSVs exactly. Work counts, roots, view ordinals, bounds and alternating resize-width parity agree across conditions. All 3,780 group-summary percentile values, 1,890 pooled percentile values and 630 same-execution summed-share values recompute exactly from raw nearest-rank observations. All 126 group/cell improvement values and the booleans in `performance-gates.json` also agree.
- The 504 formal full state files match their paired complete vectors and the original formal vectors byte-for-byte. Independent parsing checked the binary preorder/path, root/view identity, exact four-unit separators and half geometry, unique complete item ownership, source/target placement and selection intent. Independently recomputed little-endian FNV-1a digests agree. The data contain 27 state digests: 21 sample-zero cell states plus six alternate-width resize states.
- Persisted full vectors cover sample zero only (resize width 16,380). Other observations, including width 16,444, retain their digests and are checked by the probe's full outside-timer expected-state oracle. The archive must not be described as retaining every sample's full vector. Fresh owned export and complete all-root layout remain inside timing; fixture setup, expected-state validation, digest and destruction of retained results remain outside.
- All 325 inputs in the candidate source receipt match the current files. All three comparison environment source maps agree with that receipt. Frozen baseline/candidate/ImGui binary bytes, raw/summary receipt hashes and source pins match. The candidate is the `fa5fb1e` tree plus the two scoped layout files; there is no benchmark-boundary or transaction change in this slice. The independent `layout-traversal-review.md` reports no blocking source finding.
- Build and all 24 measurement processes are successful and serial: build 15:46:48–15:50:02 UTC, original comparison 15:51:11–15:51:26, checked-move comparison 15:52:42–15:52:56, ImGui comparison 15:54:22–15:55:03 on 2026-11-06. Ordinary checks begin only afterward at 15:56:07. Production timing uses opt-level 3, thin LTO, one codegen unit and disabled work counters; the ordinary verification environment restores counters. All 49 protected user files retain their starting hashes.

Frozen executable identities:

| Executable | SHA-256 |
| --- | --- |
| Original Rust | `793e8f2c166cbee6cee4885b01052d25415c9ccac06fee4189be3945de96f6d7` |
| Submitted checked move | `fc16f5bc3ad205fe6b6800440c61573295df4bca1a64e6bde1097a483044ce81` |
| Layout candidate | `72dae29c00afdf725ead3a24a1b9826e48f02314803ed4a38094bb5d4d66ab6b` |
| Real ImGui | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

## Complete-operation p50 gate

Percentages are `100 × (1 − candidate / baseline)`; positive means faster. Each entry is computed directly from the two corresponding raw group distributions, not by combining independent comparisons. Times elsewhere are microseconds. The 20% threshold is the plan/goal investment gate, not a number attributed to the user.

| Panes | Views | Case | vs original G1 | vs original G2 | vs checked move G1 | vs checked move G2 |
| ---: | ---: | --- | ---: | ---: | ---: | ---: |
| 16 | 1 | resize | +8.166038% | +1.910731% | +7.001987% | +6.583518% |
| 16 | 1 | center | +44.294623% | +46.457116% | +0.004388% | +2.158645% |
| 16 | 1 | edge | +42.783505% | +46.878400% | +5.294180% | +1.946673% |
| 16 | 4 | resize | +7.407407% | +9.820377% | +6.583518% | +1.301961% |
| 16 | 4 | center | +32.962138% | +29.209920% | +9.177680% | -2.298851% |
| 16 | 4 | edge | +28.827799% | +23.473333% | +3.043300% | -4.359091% |
| 16 | 4 | cross-view | +23.008850% | +26.295650% | -0.204052% | -3.749112% |
| 128 | 1 | resize | +11.029399% | +9.971144% | +5.917946% | +6.400688% |
| 128 | 1 | center | +47.747897% | +48.391466% | +0.635339% | -1.028193% |
| 128 | 1 | edge | +45.787863% | +44.927181% | +1.811694% | -0.019290% |
| 128 | 4 | resize | +7.350779% | +7.650909% | +7.397073% | +7.539893% |
| 128 | 4 | center | +30.367177% | +30.333571% | +2.753623% | -3.755189% |
| 128 | 4 | edge | +30.240424% | +29.083796% | +1.814184% | -0.992700% |
| 128 | 4 | cross-view | +27.818920% | +27.932648% | +1.215703% | -2.452134% |
| 1024 | 1 | resize | +6.857225% | +8.320891% | +8.580628% | +9.230988% |
| 1024 | 1 | center | +44.684560% | +44.560771% | +2.218563% | +1.954159% |
| 1024 | 1 | edge | +43.534207% | +41.766576% | +3.007900% | +1.653297% |
| 1024 | 4 | resize | +9.598093% | +8.475703% | +9.263160% | +9.296917% |
| 1024 | 4 | center | +28.272591% | +28.174245% | +1.900825% | +1.937503% |
| 1024 | 4 | edge | +28.674851% | +28.158803% | +2.342209% | +1.666624% |
| 1024 | 4 | cross-view | +27.148810% | +27.641183% | +1.703426% | +0.227777% |

All 21 cells improve in both groups against the original baseline. In particular, original-baseline 1,024-pane center improves 44.684560% / 44.560771% at one view and 28.272591% / 28.174245% at four views; edge and cross-view move improve in both groups. All six resize cells improve in both groups against both Rust baselines. Thus the original p50 investment gate and the final-candidate resize median condition close. An incremental center reduction below 20% against `fa5fb1e` is not a failure of the original-baseline gate.

Incremental smaller move cells remain mixed. The only cell with a negative p50 direction in both groups against checked move is 16-pane/four-view cross-view move. Its absolute p50s are 20.583 → 20.625 us and 21.125 → 21.917 us. The group-2 set contains eight adverse smaller-move entries; together with group-1 cross-view there are nine adverse group/cell entries total. These are not all separate repeated regressions. The report's “up to 4.36%” slowdown is correct: 16-pane/four-view edge in group 2 is 22.000 → 22.959 us. No claim of universal incremental move improvement is supported.

## Actual resize phases and attribution

Against checked move, the 1,024-pane phase medians are:

| Views | Group | Layout B / C | Layout reduction | Export B / C | Export reduction |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1 | 514.083 / 468.292 | +8.907317% | 59.625 / 51.208 | +14.116562% |
| 1 | 2 | 522.667 / 477.459 | +8.649484% | 59.667 / 52.375 | +12.221161% |
| 4 | 1 | 435.167 / 392.583 | +9.785668% | 26.333 / 26.083 | +0.949379% |
| 4 | 2 | 436.917 / 393.291 | +9.984963% | 28.209 / 28.333 | -0.439576% |

Same-sample mean deltas are additive; phase percentiles are not. The mean layout/export deltas (candidate minus checked move) are −57.664/−7.295 us and −44.847/−7.657 us for one view, and −44.122/+1.247 us and −50.090/+1.131 us for four views. Actual layout work is lower in both views/groups; four-view export means are higher and one export median is slightly higher. The result cannot be described as an across-the-board export improvement.

The code removes the separate active-path hash collection and its insert/remove work, using the existing measurement slots and row map to distinguish Unseen/Active/Completed. The former set size followed DFS depth, not total nodes. Map insertion now happens on entry, so allocation/growth/insertion order also change. Untimed initial layout can condition subsequent work. The actual timed layout reduction supports the bounded representation change on the real resize path; it does not isolate a numeric gain from one allocation, prove an allocator mechanism, or uniquely explain the historical resize shifts.

## Adverse tails retained

Compared with original, two group/cell p95 values and ten maxima are worse. Compared with checked move, eleven p95 values and twenty maxima are worse. These counts use all observations without trimming. Selected material and repeated examples follow; the full raw distributions remain authoritative.

| Baseline | Panes / views | Case | Group | p95 B / C | Max B / C |
| --- | --- | --- | ---: | ---: | ---: |
| original | 1024 / 4 | resize | 1 | 514.083 / 530.583 | 647.167 / 809.000 |
| original | 1024 / 4 | resize | 2 | 526.917 / 509.833 | 722.167 / 971.791 |
| original | 1024 / 1 | resize | 1 | 648.792 / 640.625 | 857.833 / 1,037.375 |
| original | 1024 / 4 | center | 2 | 2,583.250 / 2,199.792 | 2,864.208 / 3,189.875 |
| checked_move | 128 / 4 | center | 1 | 196.958 / 188.959 | 348.417 / 337.875 |
| checked_move | 128 / 4 | center | 2 | 209.541 / 420.042 | 623.000 / 10,296.042 |
| checked_move | 128 / 1 | resize | 1 | 64.917 / 65.542 | 148.958 / 187.958 |
| checked_move | 128 / 1 | resize | 2 | 64.958 / 64.250 | 118.750 / 150.250 |
| checked_move | 1024 / 4 | resize | 1 | 570.541 / 531.667 | 773.500 / 812.125 |
| checked_move | 1024 / 4 | resize | 2 | 596.959 / 590.917 | 1,469.041 / 865.750 |
| checked_move | 1024 / 1 | center | 1 | 2,224.958 / 2,168.709 | 2,419.334 / 2,557.459 |
| checked_move | 1024 / 1 | center | 2 | 2,246.875 / 2,175.583 | 2,501.416 / 2,630.917 |
| checked_move | 1024 / 4 | center | 1 | 1,910.250 / 1,836.250 | 2,108.667 / 2,141.667 |
| checked_move | 1024 / 4 | center | 2 | 1,981.541 / 2,013.625 | 2,234.625 / 2,646.417 |

The 128-pane/four-view center group-2 tail against checked move is a material omission in the current result paragraph: p95 doubles to 420.042 us, and maximum is 10,296.042 us. Its group-1 p50/p95/max all improve (172.500 → 167.750, 196.958 → 188.959, 348.417 → 337.875 us); do not misstate it as a two-group repetition.

The slow observation is group 2/run 4/sample 46: request 239.375, mutation 368.250, layout 9,657.917 and export 30.500 us, summing exactly to 10,296.042 us. Nearby samples also have elevated, different phases (sample 44 mutation 504.583 us; sample 45 export 215.041 us). Group-2 candidate has five observations above 400 us and one above 1,000 us, versus baseline one above 400 us and none above 1,000 us. This is recorded negative evidence. These observations alone cannot distinguish scheduling, allocator/cache conditioning or algorithmic cause. They must not be called proven noise, removed, or hidden behind a pooled median.

There was no predeclared hard maximum/p95 budget. These mixed tails and one-group stall do not by themselves demonstrate a correctness issue or invalidate the explicit p50 gate. They do require disclosure and preclude “no regression”, “all tails improve” or a frame-time upper-bound claim. No sample-selective rerun or extra optimization is required by this review.

## ImGui and abstraction boundary

The separate ImGui comparison has 192 raw observations per condition/cell. Pooled total p50s below belong only to that comparison; ratios are Rust candidate / ImGui. ImGui remains pinned to `d88f238d1d363ef387382b2e29ad12cf96f826ac` (1.93.1 WIP) and uses the unchanged actual upstream docking handlers/layout, with no renderer.

| Views | Case | Rust p50 | ImGui p50 | Ratio |
| ---: | --- | ---: | ---: | ---: |
| 1 | resize | 525.458 | 68.792 | 7.638359× |
| 1 | center | 1,907.875 | 70.000 | 27.255357× |
| 1 | edge | 2,083.375 | 110.333 | 18.882610× |
| 4 | resize | 424.250 | 67.750 | 6.261993× |
| 4 | center | 1,634.791 | 69.792 | 23.423759× |
| 4 | edge | 1,693.000 | 105.375 | 16.066429× |
| 4 | cross-view | 1,695.083 | 68.875 | 24.611005× |

For four-view center, same-execution raw phase sums / raw total sum give request 9.182824647%, mutation 64.344784052%, layout 24.735574454% and export 1.736816847%. At two decimals this is **9.18% / 64.34% / 24.74% / 1.74%**, not 64.35% mutation. The phase-share method is otherwise correct; median phase shares and subtraction of independently timed comparisons would be different quantities.

The remaining ratios measure this no-render native docking-operation boundary with common fresh owned normalized export. Rust includes atomic rollback clone, validation/canonicalization and richer projection construction. Four logical roots are not native OS viewport lifecycle; selection is command intent, not a rendered tab click. Full product frames, host settlement, font/render costs and language-only overhead cannot be inferred from these values.

## Controls and historical limits

The completed `resize-control-evidence-review.md` remains applicable. Shortened history yields one-view −0.196% / +0.316% improvement; restoring original all-size history instead yields +5.763% / +1.419%, while four views are +2.328% / −1.237%. These separately scheduled contexts do not isolate process history from wall-time/system state. The same-executable old-setup mode is 0.401965%–0.863093% slower than current setup in these repeats; the extra historical validator is neutral-to-slower. It does not support a simple claim that restoring old setup recovers the historical loss.

The setup control preserves one timed measure path and full output, but combines canonicalization/strict validation/MRU changes, shares allocator history, uses one fixed rotation, and balances ordinal counts only overall. Width × ordinal is unbalanced for the main modes (16,380 gives 32/16/0 versus 0/16/32, reversed at 16,444). Its receipt preserves private sources/binary but is narrower than the 325-input formal receipt. It cannot isolate MRU, allocator or numeric width as the historical cause. The result document accurately records these limitations, preserves the historical 2.08%/0.40% losses and does not replace formal negatives with controls.

Three controls own 7,296 raw observations and 76 sample-zero vectors. Together with the three new formal comparisons, the planned new bundle therefore has **31,488 raw observations and 580 sample-zero vectors**. Existing `fa5fb1e` archived evidence remains separate and unchanged. This report reviews source data, not yet-created archive bytes; bundle checksum/link validation remains a packaging step.

## Ordinary verification and document closure

`qualification-checks.json` and the individual logs agree: 2,024 all-feature core tests pass, three opt-in probes skip; four public-facade integration tests and nine documentation tests pass. Clippy correctness/suspicious checks, formatting, dependency boundary and diff checks return zero. Existing all/pedantic warnings remain, so “warning free” would be incorrect. All seven ordinary checks are serial and begin after measurements; no performance overlap is recorded.

The current document's seven-row 1,024-pane reductions and seven ImGui ratios round correctly. Its smaller incremental move qualification, source representation, phase-sum method, logical-view/output boundary, historical limits and width/ordinal limitation are supported. Required before publication:

1. Add the 128-pane/four-view center group-2 p95/max comparison (209.541 → 420.042 us; 623.000 → 10,296.042 us) against checked move to the tail paragraph at lines 94–102. State that it does not repeat in group 1 and its cause is unresolved. This is a report correction, not a runtime blocker.
2. Change mutation share from 64.35% to **64.34%** in the phase-share sentence. Optionally tighten the setup slowdown range at line 118 from “about 0.39%–0.88%” to “about 0.40%–0.86%”; the existing approximate bound does not alter the conclusion.

After those publication corrections, the evidence supports final p50 qualification with explicit smaller-move/tail limits. It does not support a universal latency, regression-free, native-lifecycle or unique historical-cause claim. This review requests no new runtime change or measurement.

## Hash anchors

| Artifact relative to `layout-traversal/` | SHA-256 |
| --- | --- |
| `source-receipt.json` | `38f11bb602f0eae9e34b4c22b32f26a5c252f9a9651c96e92ae5aec481225a18` |
| `compare_layout.py` | `540719d28bf333a6702fd54dccf137dda2cbf2cee9d8f2f3db9a609a51c6cb5f` |
| `performance-gates.json` | `4bb60fe8f42630a7f33bca73036eed5c621ae1def9fc1a939b39837906270239` |
| `qualification-environment.json` | `ae7d479815a67429f1e7a8255763faf9c6f465be17741c3d76a97b250b031d70` |
| `qualification-checks.json` | `cc6265ce58ae0b1d92d7cda38bf2b597fa0dd85c283d3bea81b1805aba757847` |
| `against-original/raw.csv` | `92f3a5c59d1ae43a196ae5324f1a1807db906beea644c8abc2af20a9d78194b9` |
| `against-original/summary.csv` | `7fad42c53b5ed024c8122f3b9c72950a22dc471f8c4f17700bd9353b790f6068` |
| `against-original/pooled-attribution.csv` | `a5cb30b49197c64587f8bdb7c3efa3c86a8b188e41a9a476bcc9f68fe8fb84e3` |
| `against-original/environment.json` | `0da47e1379d39a2578e308355a9cd4482d50c3686f28b54876fe557919db4e4c` |
| `against-checked_move/raw.csv` | `4bba3df71663fca8c924fe1d5ac1684d607e56a9909810f9690adb63f621c45d` |
| `against-checked_move/summary.csv` | `2a9aef9fe57a2284748991bbe656ed556320152da59a2d42889a65489241687c` |
| `against-checked_move/pooled-attribution.csv` | `2df41cf56570fa6f4fe646f3a28fdc85e23f9051a8224833d33824e94c90e2bc` |
| `against-checked_move/environment.json` | `fbcb49fb03deb8040e389cdbcbcdd34f0be97a10995c4b248d230a4877046b7f` |
| `against-imgui/raw.csv` | `a9e8faceaeabc279d18d5791d3d586cd2d81cdfb5651b712df3c96cc1f7a53c9` |
| `against-imgui/summary.csv` | `695e54daa96bf85666a6582da0c9ffebd870ddb3dcbd440ebbe166e74fffa27a` |
| `against-imgui/pooled-attribution.csv` | `b21efc972ccb670a74d7a045a36d6e49ceb6413ce06f029cb2f51fe5e247acff` |
| `against-imgui/environment.json` | `38381707b29881454df7960d1465d913996b5ac56e8019c55c2caa215b0c3561` |

Control evidence review SHA-256: `d0607e31e43dc8313de707362b2944161021a8cb900241307cc4ab52145c3d38`. Source review SHA-256: `0f907ffcef5faaebdfb37a081ea6c3b2201979e714c889f94b344d614c046d1c`.
