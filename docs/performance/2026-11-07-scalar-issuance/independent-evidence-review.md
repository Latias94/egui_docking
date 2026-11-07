# Scalar issuance evidence review

**Findings first: no blocking evidence, restoration, or decision defect found. Accept the rejection of this exact candidate.** The initial screen fails both the predeclared affected-move improvement count and minimum ImGui inventory. The passing small-move count does not override those failures. No confirmation or remaining nine qualification gates are required for rejection. This result neither establishes that allocation is free nor exhausts all possible scalar-identity designs.

This was an independent static audit on 2026-11-07. I read the archive and preserved source/binary artifacts, recomputed statistics directly from CSV observations, and compared complete output vectors and hashes. I did not invoke archived scripts, Cargo, tests, workloads, profiling, timing, network access, or change the repository. Only this report was written.

The actual Rust control is immutable `c7508eec3ac44138305797691244ae9c180d7680`, retained runtime `0e441c8ddeb6f85b9b0e8237303b5def32e178ae`. Its frozen executable is `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`. The candidate executable is `018b13f30ce6c4194cd24e58ed67d36161933a0446264002fd84da2392efeca3`; the pinned ImGui executable is `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0`, with recorded source commit `d88f238d1d363ef387382b2e29ad12cf96f826ac`. I independently hashed all three preserved binaries and matched the comparison/build receipts.

The archive contains exactly 429 regular inputs plus `manifest.json`, 430 members total, without duplicate, escaping, or non-file entries. Every input's length and SHA256 matches the manifest. All 429 also match their preserved B/source-package copies or immutable control Git objects. All seven top-level SHA256SUMS entries match. The archive is 321968 bytes, consistent with `archive-check.json`.

Both independent campaigns preserve the declared serial ABBA/BAAB orders, eight successful runs each, 21 workload cells per run, 48 observations and eight warmups per process/cell. I matched each concatenated raw CSV against its eight original run CSVs: 8064 rows per campaign, 16128 total. Every expected cell/sample occurs exactly once per run. Product labels, pane/view/case cardinalities, actual node/leaf/split/item/root counts, dimensions, positive complete durations, nonnegative phase values, phase arithmetic, and digest formatting pass independent checks.

Each group/condition/cell contains 96 observations. I recalculated nearest-rank p50, p95, and maximum for all five recorded phases: all 2520 published phase-statistic values across 168 summary rows match. All 84 comparison rows, their baseline/candidate values, ratios, reductions, and measurement aggregates match those recomputed summaries.

All 336 complete vectors are present. For each of 21 cells, all 16 vectors across campaigns, runs, and products are byte-identical. Every vector parses as complete u64 words, and independently recomputed little-endian FNV digests match the corresponding sample-zero rows. All raw digests also agree across products/conditions for each workload and actual width, including both resize widths. These are complete vectors, not digest-only equality.

Results relative to the actual Rust control:

| Complete operation statistic/subset | Improve | Regress | Tie | Reduction range |
| --- | ---: | ---: | ---: | --- |
| p50, all 42 group/cells | 18 | 21 | 3 | -8.5291% to +15.3669% |
| p95, all 42 | 21 | 20 | 1 | -26.2290% to +29.8336% |
| maximum, all 42 | 19 | 22 | 1 | -857.9478% to +80.8956% |
| p50, 30 move group/cells | 16 | 13 | 1 | Fails required 20 improvements |
| p50, 10 small move group/cells | 7 | 2 | 1 | Meets required 6 improvements |
| p50, 12 resize group/cells | 2 | 8 | 2 | All negatives retained |

The same 16-pane/four-view center case improves 15.3669% in group 1 and regresses 8.5291% in group 2. The worst p95 regression is group 2, 1024 panes/four views/edge, 105166 → 132750 ns. The largest maximum regression is group 2, 16 panes/four views/center, 2875 → 27541 ns. These observations remain in the table; no causal explanation or repeatability claim follows from the isolated maxima.

Candidate ImGui p50 inventory is 29/42: one small group/cell, all 14 medium and all 14 large group/cells meet the 2x threshold. Thus the medium/large condition passes while the minimum inventory of 30 fails. This fresh candidate campaign does not change the retained runtime's separately established 30/42. The other candidate/ImGui descriptive aggregates also reproduce: p50 2/40/0, p95 6/36/0, maximum 17/25/0 for improve/regress/tie.

Qualification scope is accurately limited. The normal-counter receipt binds 323 Rust/manifest inputs to the frozen candidate's corresponding hashes; the 324-input formal receipt additionally includes the unchanged ImGui source. Formatting passes, targeted nextest logs show 432/432 including the extended owner-reference witness, and focused regressions show 4/4. The new authority restoration witness is outside that original selection and has its own normal-counter receipt/log showing 1/1 passed before the first timing campaign. The work probe separately passes. This is three ordinary gates plus work and the additional witness, not twelve-gate qualification.

The ordinary work CSV has exactly 21 rows and 20 columns and is byte-identical to the preserved retained-control CSV. Actual MRU copies remain 4 for center/cross, 2 for edge, and 0 for resize. The six completed edge bodies total 2926 root-structure rows. No allocator, byte-capacity, or exclusive-time conclusion is inferred from those logical counters.

The candidate/control 324-input receipts differ at exactly the three declared paths: authority runtime plus its narrow unit witness, workspace's sole selection consumer, and the existing owner-reference test. Archived actual-control and measured-candidate copies exactly equal the corresponding source-package pairs and receipt hashes. The frozen runtime patch matches the previously reviewed patch exactly. Solvers, frozen reference, probe cap, complete timer, fresh leaf gathering, full owned projections, output export, work accounting, and comparison C++ source have no candidate edits.

The checked relaxed global issuer with panic-before-reuse exhaustion is explicitly documented in the predeclared scope refinement and root report; it is the authorized fail-closed choice, not an omitted recoverable error. The real no-op general Tabs transaction and the separate private owner-restoration witness retain their distinct claims. The implementation review's non-blocking wider Workspace Debug visibility caveat remains available in the archived review; the root report does not claim full Workspace Debug serial redaction.

I independently checked all 324 current formal source hashes against the retained control receipt after restoration. Every hash matches, including all three modified files. Original Arc issuance, Weak selection predecessor, and original tests are restored with no temporary scalar/test residue in that source inventory. The new root report accurately states rejection, scope, qualification, negative statistics, and unknown cause; its evidence/review links resolve.

Key SHA256 bindings:

| Artifact | SHA256 |
| --- | --- |
| `evidence.tar.xz` | `580d280cfaa6b525f56e57765f90eb77e44d1fd572167b2e0f06cc2a66cab989` |
| `comparisons.csv` | `85dffaa25e3bd9175c04a8c0c606d81d381f4cf68b0cbcd664b3adb266ebec91` |
| `measurements.json` | `31b83782443315585c4c04a007161d515cea35861b6472709f5f78498144620d` |
| `decision.json` | `bebe2d39fd3e3c8f261cc8f084a6d00a7833cc64451963cc566f29c1e82f242a` |
| `archive-check.json` | `4a22c0cfc8c35a51788e028a82fc000b6d4d416dbd2c18be6c797b5d61083de2` |
| `SHA256SUMS` | `c300d6939360dd335bc81d6155976d1c78e0d2406aac26cedaf8b03d592bfc30` |
| Root report `2026-11-07-scalar-issuance.md` | `a05d9778cb6c5ea05113939a866fc73d629bcb657db555f305a028f243d24d80` |
| Archived runtime patch | `7cd1327a27eed343e475e3435df95de2befb837a4b6a6cc51d9ca91bea18f728` |
| Archived predeclared rules | `b35a5b165450776587da431ebbffcb82715ea629309fcf6d12ad2040325b66b4` |
| Archived witness receipt | `46e383c4ec22b5e5f3a60a10cfc392841bbaa81452ac6be6aa807c516ecc50fa` |
| Archived candidate source receipt | `b6e8222b188704ccfd2ac3a6a69020ad169568d8cde7224a288d7abde648fa7e` |
| Archived work CSV | `88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4` |

Prior implementation review SHA256: `2eea680ee18ae7511951394903dc5e3d97c3d8224a39b7f46999c2d74279b9fd`. The same bytes are preserved in the top-level review and archive. Measurements are descriptive observations from this campaign; phase medians are not combined into a causal saving or performance lower bound.
