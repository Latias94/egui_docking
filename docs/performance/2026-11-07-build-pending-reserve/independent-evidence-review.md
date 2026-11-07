# Build-pending-reserve evidence review

November 7, 2026. Independent read-only audit by root_frontier_placement. Only this report was written; no build, test, benchmark, profile, runner, candidate script, or network request was executed.

**No source-contract or evidence-integrity blocker was found. Direct rejection is supported.** Affected edge p50 improves in only **6/12** cells, below the predeclared eight, and small-edge p50 improves in only **2/4**, below three. This is not a promising winner requiring the predeclared additional confirmation. No new measurement is necessary.

## Source and qualification binding

The actual control is immutable `c7508eec3ac44138305797691244ae9c180d7680`, retained runtime `0e441c8`. I independently matched all 324 control-receipt paths to its Git blobs. The candidate has the same 324 formal paths with exactly one change, graph/root_structure.rs; all 323 qualification paths match the formal candidate. Thus the rejected equal-binary and source-nonempty changes, reference changes, and benchmark/protocol changes are absent.

The saved source pair remains exactly the previously reviewed initializer-only scope: replace the single-entry pending Vec initializer with capacity `node_capacity_hint.clamp(1, 16)` followed by the same Enter push. The rest of the file is byte-identical to control. Existing hint producers, root lookup, callbacks, traversal ordering, ordinary growth, errors, output rows, and ownership remain. The capacity can overreserve a shallow/Tabs result and is not a depth or arity bound. No new test, authority, dependency, or allocator guarantee was added.

| Bound artifact | SHA-256 |
|---|---|
| Base root_structure.rs | `a89bf210b4836d10380007e1d5594ad6c76281a338b12d10f0408ba7ee98eca7` |
| Candidate root_structure.rs | `dd1451fcd1eef261cd031ef5072524556190835ff98a6adeb26d65c4c8eff1af` |
| Exact runtime.patch | `9a29292461c2b1f37ee72c6c32969cc87713fbc2645a43d191cecd9de243382e` |
| Formal source receipt | `eac222d22dabd2a3734c3ee0b44cf76f586639fe268b28500247f7ffac624f78` |
| Final checks-all.json | `bdac655dc4cb39deecb7a536584a7886ea8b97feec984ae5d3455797edefc7ce` |
| Frozen candidate executable | `e67648635ff6edf1b2c61d26b0bd78d153fc4f5ba04e105246f483981c8ae2bf` |
| Frozen Rust control executable | `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b` |
| Frozen ImGui executable | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

All twelve final ordinary gates returned zero, serially, with counters enabled and unchanged source hashes. I checked PASS ordinals and summaries: targeted 432 passed/1644 skipped; fixes 4/2072; production core 2073/3; public integrations 4/0; workspace 2249/3; host default 292/0; host serde 335/0. Formatting, Clippy, strict docs, doctests, and dependency checks also pass. Doctests report 9, 0, and 1 passed, zero failed. Clippy's recorded gate denies correctness/suspicious diagnostics while allowing other warnings.

The subsequent ordinary work test reports one passed/2075 skipped. Its 21×20 CSV is byte-identical to control, SHA-256 `88c00636e368e33a06c4e2c12df56f3adb0c53775c307d3210b190d1f832fab4`, preserving six completed bodies and 2926 structure rows. This ledger does not count pending capacity or allocator bytes.

The formal artifact records opt-level 3, debuginfo 0, debug assertions and overflow checks disabled, default features, and the normal timing-only work-counter bypass. Its existing presentation_identity_frontier dead-code warning is retained.

## Complete observations and statistics

The archive contains **426 inputs plus manifest.json**, 427 unique safe regular members. All sizes, hashes, and original-input roundtrips match. Both complete campaigns were audited, without executing their scripts:

| Campaign | UTC interval | Raw observations | Complete vectors |
|---|---|---:|---:|
| Rust control pairing | 12:00:09.249786–12:00:13.436574 | 8064 | 168 |
| ImGui pairing | 12:00:13.473964–12:00:50.529793 | 8064 | 168 |
| Total | Two separately retained campaigns | **16128** | **336** |

Each campaign has ABBA/BAAB groups, eight serial processes, 21 workloads, 48 samples, and eight configured warmups. All recorded processes return zero, stderr is empty, Rust harness output reports success, and ImGui stdout matches its CSV. Existing executable-file hashes match every environment receipt.

I independently checked every process/aggregate row, sample ordinal, structural count, alternating resize width, phase sum, and all 168 summary rows across five phases. Each summary cell has 96 observations, using sorted indices 47/91/95. Every field in all 84 comparison rows and the published statistics JSON matches recomputation.

All 336 full sample-zero vectors were parsed: root order, DFS binary paths, bounds and four-unit child gaps, selected membership, node/leaf/split counts, and every item exactly once. All 21 workload vectors agree byte-for-byte across products and runs. Sample-zero FNV digests match their dumps; all 27 workload/width digest keys agree across all 16128 observations. This does not claim a full vector dump for every timed sample.

The timed boundary is unchanged: capture, checked mutation, fresh projection of every root, and independent complete owned export. No narrow allocation count or saved-nanosecond claim is inferred from the earlier profile.

Counts are improvements / regressions / ties, based on complete operation latency:

| Comparison/subset | p50 | p95 | max |
|---|---:|---:|---:|
| Rust, all 42 | 18 / 22 / 2 | 21 / 21 / 0 | 20 / 21 / 1 |
| Rust, 12 affected edge cells | 6 / 6 / 0 | 5 / 7 / 0 | 6 / 6 / 0 |
| Rust, four small-edge cells | 2 / 2 / 0 | 1 / 3 / 0 | 2 / 2 / 0 |
| Rust, 30 nonedge cells | 12 / 16 / 2 | 16 / 14 / 0 | 14 / 15 / 1 |
| ImGui, all 42 | 2 / 40 / 0 | 5 / 37 / 0 | 10 / 32 / 0 |

| All complete cells | p50 reduction range | p95 reduction range | max reduction range |
|---|---:|---:|---:|
| Rust control | -3.905882% to +5.882353% | -116.540866% to +20.578892% | -452.787579% to +89.177917% |
| ImGui | -237.331334% to +14.033555% | -288.550323% to +16.830529% | -602.594034% to +75.722622% |

Every affected edge cell is shown below. Positive reduction means improvement:

| Group | Panes/views | Control→candidate p50 ns | Reduction |
|---:|---|---:|---:|
| 1 | 16/1 | 3125→3208 | -2.656000% |
| 1 | 16/4 | 3000→3041 | -1.366667% |
| 1 | 128/1 | 15750→15583 | +1.060317% |
| 1 | 128/4 | 12958→13125 | -1.288779% |
| 1 | 1024/1 | 119250→122292 | -2.550943% |
| 1 | 1024/4 | 93000→94375 | -1.478495% |
| 2 | 16/1 | 3125→3084 | +1.312000% |
| 2 | 16/4 | 3041→3000 | +1.348241% |
| 2 | 128/1 | 15708→15500 | +1.324166% |
| 2 | 128/4 | 13500→13208 | +2.162963% |
| 2 | 1024/1 | 121208→121166 | +0.034651% |
| 2 | 1024/4 | 92667→94416 | -1.887403% |

The small-edge group signs reverse between groups; the 1024/4 edge cell regresses in both. These observations do not establish a compiler or allocator cause. The decision does not depend on choosing a percentage definition of materiality: both explicit affected-workload gates already fail.

Adverse tails are retained. The worst Rust p95 is group-1 1024/1 edge, 132750→287458 ns (-116.540866%). The worst p50 is group-1 16/4 cross-view, 2125→2208 ns (-3.905882%); the worst max is group-2 16/1 resize, 1417→7833 ns (-452.787579%). All other p95/max negatives remain in the full comparison table; no outlier is discarded or assigned an exclusive cause.

The candidate meets the separate ImGui inventory gate at 30/42: small 2/14, medium 14/14, large 14/14. Its p50 ratios range from 1.867200x–3.373313x, 1.183333x–1.581818x, and 0.859664x–1.125279x respectively. Passing this gate cannot replace the failed Rust edge gates; the retained inventory remains 30/42.

## Decision and restoration

The predeclared rule is timestamped 11:37:30.206568 UTC, before recorded qualification and all measurement. It requires at least eight of twelve edge cells and three of four small-edge cells to improve, complete correctness/work/vector checks, the ImGui inventory, full disclosure of nonedge/tail observations, and additional confirmation only for a promising winner. No whole-matrix 28-improvement rule is substituted for these gates.

The recorded central decision at 12:02:23.887394 UTC is `reject_restore`, with the correct candidate/control identities and failed 6/12 and 2/4 gates. No confirmation is warranted under that rule, and no runtime delta should be retained.

I independently read back root_structure.rs after restoration: it is byte-identical to the saved c7508ee base. At that moment the only difference across the 324 formal source paths is a separate authority.rs work-in-progress for the next candidate. That live change was not used as pending-reserve source evidence. The central receipt records the preceding full 324-source restoration; the directly checked reverted path confirms that this candidate's initializer is absent. No qualification or benefit is claimed for the next candidate.

| Durable evidence | SHA-256 |
|---|---|
| Predeclared rule | `a6e0bf4ee72266950aa119253a5f8519c5e66e857b33e26c057f417adcc77130` |
| evidence.tar.xz, 345136 bytes | `82caad3e0911a68baf847607e735a06e4e734349a4163393a4839fcd0f9054a5` |
| Archive manifest.json | `fbaa5ca496f2ecb0e488e2bed082b9320f9e5602b4f0001e99ec7b48f2e01d6e` |
| archive-check.json | `2febb63fd9a70ca909bb943d33950bb67a4d9dbd43923566a544e5cedbabace9` |
| comparisons.csv | `6df1ecbd4bd32890417dd3295c9c2412657b11f353cd88cd8842ed6f073e9300` |
| measurements.json | `fa90001d5885f67a97e0a831f0c4f8e673d8eaaaa7a092074ae74230ebf4bb6f` |
| Prior source-scope review | `d21a2ea708407536723bff4423932f7b96b250c9acb48f477e147b4ffe201dfe` |
| SHA256SUMS | `53cde6b4811402f10aeca24e2bbd799eb21af8a3fcaa630456acb907069fd65c` |
| Later central decision.json | `e64fd74bf8d885758d3f22bca9f21795e6dc7ac882bc39d31fa7f707de43f2d9` |

All five SHA256SUMS entries match. The later decision is separately hashed above. This report binds the reviewed evidence snapshot, not an unprovided publication commit. The blocker to retention is the failed predeclared edge benefit; correctness and evidence integrity pass.
