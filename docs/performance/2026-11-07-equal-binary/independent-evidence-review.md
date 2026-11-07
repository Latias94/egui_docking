# Equal-binary evidence review

November 7, 2026. Independent read-only evidence review by root_frontier_placement. I parsed the saved CSVs, complete vectors, receipts, source pairs, Git objects, and archive; I did not run Cargo, tests, benchmarks, profiles, candidate scripts, archived runners, or network requests. Only this report was written.

**No source-contract or evidence-integrity blocker was found. The evidence supports the central reject/restore decision.** The initial campaign passes the quantitative gates, but its favorable complete and small-layout counts do not repeat in the single additional full Rust campaign. Some adverse tails persist, while others reverse. This supports rejecting this attempted optimization, not claiming that it universally worsens performance.

## Binding and correctness

Actual control: immutable `c7508eec3ac44138305797691244ae9c180d7680`, with retained runtime `0e441c8` and the existing frozen executable below. I independently matched all 324 control-receipt source hashes to its Git blobs. The candidate formal receipt contains the same 324 paths; only layout.rs and layout/topology_tests.rs differ. All 323 qualification paths match the candidate formal receipt. The additional formal path is the unchanged C++ comparator.

| Artifact | SHA-256 |
|---|---|
| Rust control executable | `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b` |
| Candidate executable | `fc17c426ae6c25d747a2231807d187866c30bedde35cdb32c9abd933978998a6` |
| ImGui executable | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |
| Base layout.rs | `01f21abad6c4d71856dbf17ae9eec90a97d7fbe7717bb2e6da87895f2fed5dcd` |
| Candidate layout.rs | `69d76d26c66b61f7fa9a19068a01ed248522d155d701e6e43a90c4d4d52e1ef6` |
| Base topology_tests.rs | `67671850120413fbaec896b7eea930b7eb52086ccf4864b8486ab21ed8ea1ad2` |
| Candidate topology_tests.rs | `41ffce25e6766bb49bff1ffc31be8cbe28f2cb97a370307a306498bf18222b57` |
| Exact runtime-and-test patch | `aad30c7ad464608ebd73027b8715b53617726479924d77716ea5991bdf2a4896` |
| Formal source receipt | `120326559d4db74f471515a54cd2dc57fdc8e22a9bbac4577e3d39764ba384c4` |
| checks-all.json | `92710fda4c0738f29ae3af599c0b749956797dc0149fb4fa6be75bb15d78fe64` |

Independent patch reconstruction produces exactly two insertion-only hunks: eight runtime lines and 186 test lines. The runtime branch is inside the original desired_sizes_into, after the unchanged central branch has returned. Exactly two equal weights use `sizes.fill(extent * 0.5)`. The original validator still rejects nonfinite/negative weights, zero noncentral totals, and overflowing totals before this point; minimum/maximum aggregates and infeasible returns also precede it. Equal accepted binary weights have an exact half share, including the positive subnormal boundary. The original clamp, assigned sums, correction, outcome calculation, central arithmetic, and unequal/n-ary fallback remain. No second solver, scalar-ABI change, new authority, or removed raw validation is present.

The frozen reference is unchanged, SHA-256 `4b6ff84c9bed4b65a202e1c771b5f176b161bbb27b99dba25e4875c7b08c07c6`. The added registered witness performs **31 oracle calls in one test**, not 31 separately registered tests. It compares size, overflow, and unallocated bits, handles NaN error bits explicitly, and checks signed zero, subnormal/large finite boundaries, clamp/correction behavior, infeasible outcomes, unequal/central/n-ary fallback, and conflicting-error precedence. Existing tests are not weakened or replaced.

The twelve ordinary gates ran serially on the final candidate, with counters enabled and unchanged source hashes: formatting; targeted 433 passed/1644 skipped; fixes 4/2073; production core 2074/3; public integrations 4/0; workspace 2250/3; Clippy; strict documentation; doctests; dependency checks; host default 292/0; host serde 335/0. I checked every saved nextest PASS ordinal and summary. The new oracle test appears as PASS in targeted, production, and workspace logs. Doctest summaries are 9, 0, and 1 passed, zero failed. Clippy denies correctness/suspicious diagnostics while allowing other warnings; documentation uses warnings-as-errors.

The ordinary docking_work run passed one test, with 2076 skipped. Its 21 rows × 20 columns are byte-identical to the retained control ledger, SHA-256 `88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4`, including six completed bodies and 2926 structure rows. This is logical-work equality, not an allocator-byte claim.

The formal candidate artifact has opt-level 3, debuginfo 0, debug assertions and overflow checks disabled, default features, and the recorded work-counter bypass used only for timing. Qualification precedes the formal build. The existing presentation_identity_frontier dead-code warning remains; no warning-free claim is made.

## All campaigns and archive

The final archive has **628 input files**, plus manifest.json: 629 unique, safe regular members, with every length/hash and roundtrip to its original input verified. This supersedes the preliminary 431-input package by retaining the confirmation and its associated evidence; it does not replace the initial observations.

| Campaign | UTC interval | Raw rows | Complete vectors |
|---|---|---:|---:|
| Initial Rust pairing | 11:30:08.550215–11:30:12.690617 | 8064 | 168 |
| Sole ImGui pairing | 11:30:12.728057–11:30:49.667643 | 8064 | 168 |
| Additional Rust confirmation | 11:35:13.303912–11:35:17.305678 | 8064 | 168 |
| Total, kept separately | | **24192** | **504** |

Each campaign preserves two groups, ABBA and BAAB, eight processes, 21 workloads, 48 samples, and eight configured warmups. All 24 recorded process intervals are serial. All return codes are zero, stderr is empty, Rust harness stdout reports success, and ImGui stdout equals its saved CSV. Each 1008-row process CSV is an exact slice of its aggregate. Each campaign binds the same frozen candidate and appropriate control hashes; I also hashed the existing executable files.

I checked sample ordinals, workload cardinalities, structural counts, alternating resize widths, phase sums, and all 252 summary rows across five phases. Each cell contains 96 observations, with sorted indices 47/91/95 for p50/p95/max. All 84 initial comparison rows, all 42 confirmation rows, and both measurement JSON summaries match independent recomputation. No campaign was pooled with or substituted for another.

All 504 exported vectors were parsed, checking the DFS forest and root order, binary paths, bounds and four-unit child gaps, selected membership, exact node/leaf/split counts, and each of the 2N items exactly once. All 21 workload vectors agree byte-for-byte across products, runs, and campaigns. Recomputed sample-zero little-endian-u64 FNV digests match; all 27 workload/width digest keys agree across the 24192 observations. Vector dumps are for sample zero; other samples retain their digest and structural checks.

The unchanged clock includes capture, checked mutation, fresh projection of every root, and independent complete owned export. Fixtures and output destruction remain outside the same boundaries as control. The original post-fixture-drop output assertion and rich-output validation remain.

## Results, including adverse observations

Counts are improvements / regressions / ties, using complete operation latency unless a row explicitly says layout.

| Campaign or subset | p50 | p95 | max |
|---|---:|---:|---:|
| Initial Rust, all 42 | 34 / 8 / 0 | 20 / 22 / 0 | 24 / 18 / 0 |
| Initial Rust, small complete | 13 / 1 / 0 | 6 / 8 / 0 | 11 / 3 / 0 |
| Initial Rust, medium complete | 13 / 1 / 0 | 10 / 4 / 0 | 9 / 5 / 0 |
| Initial Rust, large complete | 8 / 6 / 0 | 4 / 10 / 0 | 4 / 10 / 0 |
| Confirmation Rust, all 42 | 20 / 20 / 2 | 21 / 21 / 0 | 21 / 21 / 0 |
| Confirmation Rust, small complete | 4 / 9 / 1 | 7 / 7 / 0 | 9 / 5 / 0 |
| Confirmation Rust, medium complete | 9 / 4 / 1 | 8 / 6 / 0 | 5 / 9 / 0 |
| Confirmation Rust, large complete | 7 / 7 / 0 | 6 / 8 / 0 | 7 / 7 / 0 |
| ImGui pairing, all 42 | 4 / 38 / 0 | 6 / 36 / 0 | 12 / 30 / 0 |

The small **layout** p50 gate is distinct: initially 13 improvements/1 regression/0 ties, with reduction range -4.100000% to +12.500000%; confirmation has 6 improvements/5 regressions/3 ties, range -11.900192% to +4.384134%.

| All complete cells | p50 reduction range | p95 reduction range | max reduction range |
|---|---:|---:|---:|
| Initial Rust | -4.064643% to +10.000000% | -21.052632% to +71.068340% | -336.502747% to +88.250888% |
| Confirmation Rust | -13.636364% to +6.870861% | -123.231890% to +66.038761% | -726.086957% to +77.732957% |
| ImGui | -246.720000% to +15.090090% | -466.600000% to +21.076205% | -1607.717250% to +90.361087% |

Every initial Rust p50 negative is retained below. Reduction is positive for improvement. The final column is the corresponding confirmation cell, not a selected replacement.

| Group | Panes/views | Case | Initial control→candidate p50 ns | Initial reduction | Confirmation reduction |
|---:|---|---|---:|---:|---:|
| 1 | 128/1 | edge | 14916→15334 | -2.802360% | +2.571487% |
| 1 | 1024/1 | resize | 70541→70834 | -0.415361% | -2.777273% |
| 1 | 1024/1 | edge | 120041→121000 | -0.798894% | -1.984130% |
| 1 | 1024/4 | resize | 72125→72250 | -0.173310% | +3.989060% |
| 1 | 1024/4 | edge | 94750→95208 | -0.483377% | -2.002861% |
| 1 | 1024/4 | cross-view | 78541→79042 | -0.637883% | -1.754839% |
| 2 | 16/4 | cross-view | 2042→2125 | -4.064643% | -2.145046% |
| 2 | 1024/1 | edge | 119209→119458 | -0.208877% | -0.674284% |

Initially five workload configurations have negative p95 in both groups. Examples include 1024/4 center at -16.509445%/-8.464479%, 1024/4 cross-view at -8.876046%/-4.529314%, and 16/1 edge at -9.889241%/-8.149595%. These are the reason a full tail confirmation was appropriate; the median win count alone did not resolve them.

The confirmation is mixed. For 1024/4 center, p95 changes by **+4.259379% / -8.361837%**, while max remains negative in both groups: 99791→157416 ns (-57.745688%) and 214250→292250 ns (-36.406068%). For 1024/4 cross-view, confirmation p95 is -2.533532%/+3.001739%, and both maxima improve. For 16/1 edge, confirmation p95 is -4.219303%/+2.290701%, and both maxima improve. It would be incorrect to describe all initial tail losses as persisting.

The initial worst max is group-2 128/1 center, 15292→66750 ns. Confirmation's worst p50 is group-1 16/4 edge, 2750→3125 ns; its worst p95 and max are group-1 16/4 cross-view, 2333→5208 ns and 2875→23750 ns. All p95/max values and adverse cells remain in the complete comparison tables. These observations do not establish an exclusive compiler, allocator, or scheduling cause.

The sole ImGui campaign has 31/42 cells at or below 2x: small 3/14, medium 14/14, large 14/14. Candidate/comparator p50 ratios range from 1.750751x–3.467200x, 1.172133x–1.509091x, and 0.849099x–1.108392x respectively. This is the rejected candidate's inventory; the retained control remains at 30/42. No new ImGui campaign was run for the Rust confirmation.

## Decision, restoration, and durable binding

The predeclared file is dated 11:06:37.895268 UTC, before recorded qualification and all measurements. Its thresholds are unchanged: at least 28 complete p50 improvements, at least eight small-layout p50 improvements, at least 30/42 ImGui cells with every medium/large cell at or below 2x, complete correctness/work/output preservation, and disclosure of tails with repeatable material negative tradeoffs blocking retention and no selective campaign.

The initial 34/42 and 13/14 pass. Confirmation 20/42 and 6/14 do not reproduce those benefits. Keeping only the favorable initial campaign would disregard the confirmation; several tail concerns also remain unresolved. This supports the recorded central `reject_restore` decision without inventing a new percentage cutoff or claiming statistically proven causality. The first campaign remains valid evidence and is not rewritten as a failure of every workload. No fourth campaign is needed or requested.

Root's decision records restoration of all 324 formal control sources and removal of the added witness. By the time of this audit, a separately authorized pending-stack initializer experiment was already present. I independently checked that current layout.rs and topology_tests.rs have returned to the exact control bytes; all 323 formal paths other than root_structure.rs match the immutable-bound control map. That separate experiment's saved root_structure base also matches the remaining control path, and its current file is exactly its initializer-only candidate. This confirms the complete control-source composition and exclusion of equal-binary from the next candidate; it does not use the new live experiment as equal-binary evidence or claim its qualification.

| Durable artifact | SHA-256 |
|---|---|
| Predeclared rule | `5f6bdfeb403e32e6689fa3379682c753c6e02ff8f32238188de0f41eeaf6ea34` |
| Final evidence.tar.xz, 482380 bytes | `f50886c2b86545dbcd5745456f0be70c69d30eb217acab4d0ebca4fab626344a` |
| Archive manifest.json | `a5ddb3c69c60fa4c278b24a7c2e5e2d32172863cc856758656cd299334eb2034` |
| archive-check.json | `33b19b983daa4ef6fa6fb0900946666917db05f64af25a6c8114ee76e97a2ff6` |
| Initial comparisons.csv | `7d7fe0e5c070d3d0230f09f80f733c6af53f3688c11d165657b9f59f4b8cdd90` |
| Initial measurements.json | `d746c68600d72865d57a2ea97bb0c444f239626f922258519f373f12aca5963b` |
| Confirmation comparisons | `45498b0c49181bd8eb619436a33a2617e381650ffc0311eedf42bead265bf27a` |
| Confirmation measurements | `059ad92438487d4eadb768edf2d0f7da2bc5bf1b4f18159d32e6a06e0d7346ed` |
| Central decision.json | `19f4a42922d4eba3adebb6af43003dcc22215d79df13ccc3ae113906b0dde757` |
| Source review, hashed for binding | `a20e141f6decd6300a47d4a1c708639c19ef2cd3dd747b25bf10949b608c7088` |
| SHA256SUMS | `608a27b2c709960e5ea133a22257adfba3e07f9c1f9fbff548b5376f59628ca3` |

All seven SHA256SUMS entries match. The later central decision is separately hashed above and is not one of those seven entries. This report binds the completed evidence snapshot, not a later publication commit. There is no correctness or archive blocker; the blocker to retention is insufficiently repeatable performance benefit with unresolved adverse tails.
