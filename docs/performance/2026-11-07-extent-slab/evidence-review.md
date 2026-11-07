# Independent evidence review: rejected child extent slab

No evidence blocker was found. The archived source/binary identities, ordinary checks, complete work counters, raw comparisons, negative results and rejection/restoration decision agree with `docs/performance/2026-11-07-extent-slab.md`. The data support rejecting this exact slab experiment and keeping the 28/42 control. They do not establish an allocator/CPU cause, a performance lower bound or completion of the overall target. This review independently recomputes existing evidence; it runs no benchmark, test or archived runner.

The review is bound to immutable control `e3c517195ddce2c7d1a6dbfd4328f152978968ec` and these artifact hashes:

| Artifact | SHA-256 |
| --- | --- |
| Performance document | `46caafcf300595c76c436844091800f4d3cbb46930bc660c84e166acd1b52510` |
| `evidence.tar.xz` | `91043cc65dabea3d64f29547640ac08d722faac6a28b51824ec9672e43582d99` |
| `checks.json` | `3ab5400b4d07033af1aa254e43be7348f8dd21eafe00af2705a48d1e6fabc355` |
| `comparisons.csv` | `78aac6d5a00e934c27d221e49e7c21af3ca45aa5a1639e2d451ac2a678f6ba4d` |
| `decision.json` | `5aefbb1e24fc43fcbe6df00de10d43732d97d918e12c525659d441b5e3c0423a` |
| Frozen measured `runtime.patch` | `1d8d6fd5418b393b48e8f3a008f440ed3b076fe2533862f57723a3805fed1bf5` |

The archive has 431 input members plus its manifest, all unique regular files with safe relative names. Every member's full SHA-256 and byte count matches the archive manifest, and all seven outer SHA256SUMS entries match. The 426 original temporary inputs still present outside the archive also match byte for byte; the five remaining inputs are the immutable two-source control pair, measured pair and control commit text, independently checked below. No member was extracted or script executed.

Source and binary binding is coherent. All 324 control-source/input hashes in the archived row-capacity freeze equal the immutable e3c5171 objects. Candidate and control receipts differ only in these two runtime files; all 323 ordinary-check source/input hashes agree with the candidate freeze. The benchmark source, manifests and pinned C++ driver are shared, so this candidate does not change workload or timer boundaries.

| Source | Actual control SHA-256 | Measured slab SHA-256 |
| --- | --- | --- |
| `crates/dockspace/src/layout.rs` | `325f56f16cacbde46cce531461c8830417a6aa8ff4af06f6c87145a7528963df` | `e4597343ab8b0f8d69f94bebf7e23e190905dace9e77b942b2187c2ef555d26c` |
| `crates/dockspace/src/layout/projection.rs` | `d86c008dd78c5f1ed805de76b71d102a29dd3dab4e6ee37ca8f3cb74839edfb0` | `d770e03663941e840e5e37222d9663092c3e699f27b51dee535d301abc492c60` |

The measured runtime patch and original temporary candidate patch both reconstruct exactly that measured pair in memory. The same frozen runtime patch reversed against the measured pair reconstructs exactly the immutable control pair. The candidate build JSON records one dockspace test executable artifact and successful build; the source receipt reports unchanged inputs and the formal counter bypass. The actual still-frozen executable files were read and hashed, without execution:

| Executable | SHA-256 |
| --- | --- |
| Rejected slab candidate | `22a1457de155c62b66c445c2a23b549b149e59172ed830f279333d409532124e` |
| Actual row-capacity control | `be863f934c761071b9ebc206a0a328941f9d8a12f75e2cb9a8119470e0784a6e` |
| Pinned ImGui driver | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

Both comparison environments name those same hashes, record unchanged frozen binaries and source identities, and contain zero exits for all sixteen process runs. These receipts and frozen bytes bind the existing measurements; this review does not reproduce the build or claim that binaries themselves are embedded in the source/log archive.

All twelve ordinary gates have zero returns and matching logs: 428 focused tests, four work regressions, 2,069 production-profile core tests, four public integrations, 2,245 workspace tests, 292 default host tests, 335 serde host tests, formatting, correctness/suspicious Clippy, strict documentation, ten doctests and dependency boundaries. Three existing opt-in probes remain skipped in the full core/workspace gates; focused filtering is not extra executed coverage. Ordinary verification explicitly removes the counter bypass, while only the separately frozen timing build enables it. No result is inferred from a formatting-only or compilation-only check.

The additional ordinary docking-work probe also passes in its archived receipt. Its complete CSV has 21 unique cells and 20 columns, equals the retained control's CSV byte for byte, and matches the separately archived row-capacity evidence. It reports six successful root bodies and 2,926 rows; each edge_split cell records one completion and the other cells none. These are logical-work counts and preserve the counter contract. They are not allocation events, capacity bytes or an estimate of the slab's allocation saving.

The raw protocol was independently checked in memory. Each comparison has serial ABBA then BAAB groups; each group has two processes per condition. Every process has 48 measured samples for each of 21 valid cells, with eight warmups declared by the preserved driver/environment. Thus each condition/group/cell has 96 observations. There are 8,064 raw observations per comparison, 16,128 total, and 168 summary rows. The raw aggregates exactly reproduce the per-process CSVs. Every sample has the expected product, unique sample key, complete node/leaf/split/item/root counts, alternating resize bounds, nonnegative phase times and exact complete-duration phase sum.

All 336 preserved complete vectors parse into complete exported records with the required counts and item ownership. For each of the 21 workload cells, their bytes agree across conditions, groups, processes and both products. Each dump is the first measured sample at its cell; its little-endian u64 FNV digest was independently recomputed and matches that raw sample. All raw digests also agree for each cell and actual width, covering the alternating resize inputs. The measured interval still spans request capture, checked mutation, fresh all-root leaf input/layout and fresh complete owned export; fixture setup, independent validation, digest, I/O and final rich-output disposal remain outside it as documented.

I independently recomputed all five phases' p50, p95 and max for every condition/group/cell using the preserved nearest-rank rule (ceil(n*p/100)-1). Every summary field matches. All 84 comparison rows, including all 42 group/cells for each reference, reproduce comparisons.csv and decision.json exactly. Complete-operation outcomes against the actual control are:

| Statistic | Improve / regress / tie | Reduction range |
| --- | --- | --- |
| p50 | 1 / 40 / 1 | -12.63448% to +1.43984% |
| p95 | 8 / 33 / 1 | -41.40606% to +35.05128% |
| max | 19 / 22 / 1 | -515.24578% to +89.32511% |

The sole p50 improvement is group 1, 16 panes / 4 views / center_move (+1.43984%); the p50 tie is group 1, 16 panes / 4 views / edge_split. All fourteen 1024-pane group/cells regress in p50, by 3.01438% to 9.59532%. The substantial negative p95/max results are retained, not omitted because some tail cells improve.

Against ImGui, all 42 p50 and all 42 p95 cells are slower; max has 9 improvements and 33 regressions. The complete p50 threshold result is:

| Panes | Candidate / ImGui p50 range | At most 2x |
| --- | --- | --- |
| 16 | 2.7344x to 4.8672x | 0/14 |
| 128 | 1.62539x to 2.05993x | 11/14 |
| 1024 | 1.07737x to 1.49462x | 14/14 |

That is 25/42 for the rejected candidate. The separately preserved row-capacity ImGui summary independently counts 28/42 for the retained control. This is a frozen result comparison, not a claim about an unmeasured later head. These overlapping observations do not establish architectural necessity or the cause of the regression. Extra range operations, generated code and layout remain hypotheses, not exclusive attribution.

Rejection/restoration boundaries are explicit. decision.json rejects this exact two-source patch, binds the candidate binary and runtime patch, and records both restored source hashes equal to e3c5171. The archived control pairs, immutable objects and independent reverse reconstruction corroborate those restoration identities. This review does not treat the mutable current workspace as either the rejected slab or an untouched control: subsequent child-row work is already a separate lane. The rejection applies to the measured slab; it does not reject unrelated prerequisite reviews or authorize retaining some of the slab silently. The overall active target remains incomplete at the retained 28/42 result.

Only `/tmp/extent-slab-evidence-review.md` was written. No Cargo, compiler, tests, benchmarks, timers, runners or archived scripts were executed. No repository/fork/cache/candidate source or existing evidence was changed; no sub-agent or network was used. Read-only archive data, immutable Git objects, frozen executable bytes and independent arithmetic supplied this review.
