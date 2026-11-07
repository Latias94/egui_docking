# Independent evidence review: rejected child-row composition

No evidence blocker was found. Independent recomputation agrees with the performance document, all 84 comparisons, the ordinary checks/work receipts, and the decision to reject this exact child-row candidate and retain the control. This is an evidence review of existing records, not a new build, test, timing run, allocator analysis, or approval of a later WIP head.

## Immutable identity and archive

The document and every outer evidence file match immutable commit `3bf41ecd804a7cff4c5f091cc22b7b7f75cbebf8` (`3bf41ec`). The actual control is `e3c517195ddce2c7d1a6dbfd4328f152978968ec`.

| Artifact | SHA-256 |
| --- | --- |
| `docs/performance/2026-11-07-child-rows.md` | `6544a905f013e0c82069c8096647881a3b1e4d46540cbc5eb4425cb466c4faa2` |
| `evidence.tar.xz` | `532abf11a8a703f6729393a7ea48f3d2ecff3a247df61740d3f60f0a61828621` |
| `checks.json` | `aa795cb7b965aeef448c45ceb9ced110614c92f83c3e130dad375ae3055786ce` |
| `comparisons.csv` | `56f42586dcd90daf788c5c4da8c469e913c479f6c5d6f52db2a020b9c4af901b` |
| `decision.json` | `ca202f8bb083894a0b4a392e303b5b84ac92966a492030b70599b29300ea59f6` |
| Frozen `runtime.patch` | `9909e84c05fc98d4e59732700fc829dd7902490b6421aa10e3ed1a949877d90b` |

All eight outer SHA256SUMS entries match. The archive contains 434 inputs plus its manifest: 435 unique, safe relative, regular members. Every complete SHA-256 and byte length matches the manifest. The 427 original temporary inputs still present also match byte for byte; the other seven are the immutable control commit text and three complete control/candidate pairs checked below. No archive member was extracted and no archived script was executed.

## Source and binary binding

All 324 input hashes in the archived control freeze match immutable e3c5171 objects. The candidate receipt differs only in the three expected files. All 323 ordinary-check source/input hashes agree with the candidate timing freeze; benchmark source, manifests, lockfile and comparison driver input are unchanged. The archived complete pairs match their respective receipts.

| Source | Actual control SHA-256 | Measured candidate SHA-256 |
| --- | --- | --- |
| `crates/dockspace/src/layout.rs` | `325f56f16cacbde46cce531461c8830417a6aa8ff4af06f6c87145a7528963df` | `9ba215565d486e56d7c5435e123691948fece5a25ff5479bb245f793c4ab989f` |
| `crates/dockspace/src/layout/reference.rs` | `2b2eee151d88bdd83048f0dfe7a4931a769aff6616da59e060200819008b209a` | `ea4afaf3fc82be4e3f27f8c00aa558d5aed0ff37ff15a184d3db3dc49f7e7b27` |
| `crates/dockspace/src/layout/topology.rs` | `d2d06077b9a5bfb03f764b0ed9c862de4e9aca3a65946059e480b8412f356f56` | `89e94dc277cbfe3d4734ddd16f25fffee4c735b5144f75fd3e4b119006ece9c9` |

Independent forward application of the frozen runtime patch in memory reconstructs those exact candidate bytes. Its reverse reconstructs all three exact control bytes. The original author patch (`3f826644008677c59c44917c4f38dda5542ccd3ccdca286d379c572a3a34322e`) also reconstructs the same measured pairs when its uniquely matching hunks are applied to the actual control; its added/deleted bodies equal the runtime patch. The original author's complete layout base included the old slab, but the measured pair is the retained three-vector rebase. `layout/projection.rs` remains unchanged control in the 324-input comparison, so no slab is hidden in this measurement.

The archived build JSON reports exactly one production dockspace test executable and successful build, with unchanged-source/counter-bypass receipts. I read and hashed the still-frozen executable files without executing them:

| Executable | SHA-256 |
| --- | --- |
| Rejected child-row candidate | `93b6c26d8b47b42bf34a0867a7766836f5881a49a75afdea4a415a284661d6df` |
| Actual row-capacity control | `be863f934c761071b9ebc206a0a328941f9d8a12f75e2cb9a8119470e0784a6e` |
| Pinned ImGui driver | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

Both comparison environments identify these same binaries, the exact candidate input set and the intended control/ImGui pin. Their raw/summary result hashes, unchanged-binary flags and sixteen zero-exit run records agree. These receipts bind the recorded measurements; this review does not rebuild the executables or imply they are embedded in the source/log archive.

## Ordinary checks and work

The twelve ordinary checks all have zero exit status, matching the preserved logs: 428 focused tests, four work regressions, 2,069 production core tests, four public integrations, 2,245 workspace tests, 292 default host tests, 335 serde host tests, formatting, correctness/suspicious Clippy, strict docs, ten doctests and dependency boundaries. Full core/workspace logs retain three skipped opt-in probes. Focused filtering is not extra executed coverage. The read-only qualification-script inspection confirms normal verification removes the timing counter bypass and sets strict documentation warnings; only the separate frozen timing build enables the bypass.

The additional ordinary docking-work probe has a zero-exit receipt and passing log. Its CSV has exactly 21 unique workload cells and 20 columns. It is byte-identical to the actual control's original work.csv and the separately preserved row-capacity archive. The totals are six successful completed root bodies and 2,926 rows; every edge cell has one completion and other cells none. These are logical work counters, not allocation events, capacity bytes, or evidence that removing the gather saved allocator time.

## Raw samples, complete outputs, and statistics

Each comparison has ABBA then BAAB groups, two processes per condition/group, 48 measured samples per process/cell, and eight declared warmups. Run timestamps retain serial ordering. There are 8,064 raw rows per comparison, 16,128 total, with 96 observations per condition/group/cell. The aggregate raw rows reproduce every per-process CSV exactly. Every record has the expected product, unique key, complete node/leaf/split/item/root counts, alternating resize input bounds, nonnegative phases and exact phase-sum total.

All 336 complete vectors parse into complete records with unique root/path keys, valid DFS structure, expected counts, selected-item membership and exact item ownership. Each workload cell's bytes agree across conditions, processes, groups and products. I independently recomputed little-endian u64 FNV for every dumped vector; each equals its cell's sample-0 raw digest. All raw digests agree by cell and actual width, including alternating resize widths.

All five phases' p50, p95 and max were independently recomputed from the raw rows using nearest rank, `ceil(n*p/100)-1`. All 168 summary rows and all 84 complete-operation comparison rows reproduce exactly, including the decision statistics. Against the actual control:

| Statistic | Improve / regress / tie | Reduction range |
| --- | --- | --- |
| p50 | 0 / 41 / 1 | -22.63704% to 0% |
| p95 | 3 / 39 / 0 | -39.95201% to +41.07907% |
| max | 13 / 29 / 0 | -352.69333% to +92.21148% |

The sole p50 tie is group 1, 16 panes / 4 views / resize. All fourteen 1024-pane group/cells regress in p50, by 4.33213% to 12.24201%. Positive tail cells do not cancel the retained negative observations or provide a whole-operation p50 benefit.

Against ImGui, all 42 p50 and p95 cells are slower; max has eight improvements and 34 regressions. The candidate's complete p50 threshold is:

| Panes | Candidate / ImGui range | At most 2x |
| --- | --- | --- |
| 16 | 2.8656x to 4.8x | 0/14 |
| 128 | 1.60157x to 2.06717x | 12/14 |
| 1024 | 1.12173x to 1.53908x | 14/14 |

This reproduces 26/42 for the rejected candidate. I separately recomputed the preserved row-capacity archive's ImGui summary and confirmed retained control is 28/42. The complete target remains active and unmet.

The unchanged measured operation includes request capture, checked mutation, fresh all-root leaf inputs/layout, and fresh complete owned export. Fixture setup, independent validation/digest/I/O and final rich-output disposal remain outside that interval as documented. Removing a temporary gather can be offset by indexed rereads, bounds checks, locality or generated code; none of those explanations is exclusively established by this data. No allocator time share, architectural lower bound, or source-level allocation claim follows from these counters.

## Rejection and restoration boundary

The decision rejects the exact three-source child-row patch, binds its binary/runtime patch, and records every restored source hash equal to e3c5171. The archived control pairs, immutable Git objects and independent reverse reconstruction corroborate those exact restoration identities. The receipt describes the historical restoration action; current mutable layout WIP is deliberately not used as proof of that earlier state. Subsequent compact-frame qualification is a separate candidate. The rejection does not silently retain part of childrows, reject unrelated prerequisites, or make a later head measured.

Only `/tmp/child-rows-evidence-review.md` was written. No Cargo, compiler, tests, timing, benchmark, runner, archived script, network, sub-agent, repository/fork/cache/candidate write, or existing-evidence mutation was performed. Read-only immutable Git/archive bytes, frozen binary hashing and independent arithmetic supplied this review.
