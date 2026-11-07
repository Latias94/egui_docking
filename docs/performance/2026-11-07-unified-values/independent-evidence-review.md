No evidence blocker was found. Independent recomputation agrees with the immutable document, decision, complete observations, source/binary bindings, twelve ordinary gates, and work counts. The measured retained variant has 39 improving complete p50 cells, two regressions, and one tie. The earlier commentary count of 40 improvements is not the recorded result. The evidence supports the stated bounded retention decision, with adverse tails and the incomplete target visible.

This is an independent evidence review of immutable `07f566ac8fbddfab819f46f613d55bc2069507dd` against actual control `59a498086caee70ec9e3d2403711f16bfff6dc09`. The current axis WIP was not used as either condition's source. The previously completed semantic source review remains a separate record; this review does not replace it with another full semantic audit.

| Artifact | SHA-256 |
| --- | --- |
| `docs/performance/2026-11-07-unified-values.md` | `25f8f92f948d9729b776983e448c29fe7e1ad1b7a8b04ac054b639e077ec4ea4` |
| `evidence.tar.xz` | `5321e12cf5cdba14cec87f0c85e4ebe734b13296c8df47c12744897429b9c3bd` |
| `checks.json` | `236b5b20d94acad65e069c8e2c416e8b4e71283b5c3bffc41272f405d700339b` |
| `comparisons.csv` | `d39a708818ca826058e1e8cf12fb3df3e0f90bfafb009ce37ebd5d6fca07de6f` |
| `decision.json` | `54435b59788a20d164772f0a92673136f244f589308936b19aefc530b219690c` |
| Frozen measured `runtime.patch` | `6c61307a004fe57ff9252151ca7a74faddf15ee4d336f64b42ab647a44890a49` |
| Reviewed original `candidate.patch` | `f866a91d48521dec4931fac1b0e2002ee237cb5b86e39d5ff03c0390af184ea1` |

The document and all outer evidence files match immutable 07f566a byte for byte. All eight SHA256SUMS entries match. The archive contains 439 inputs plus its manifest: 440 unique regular files with safe relative paths. I recomputed every member's full hash and length against that manifest without extracting or executing anything. All 430 original temporary inputs also remain byte-identical. The other nine inputs are the immutable control commit text and four complete control/candidate source pairs.

All 324 candidate timing input hashes match immutable 07f566a; all 324 control input hashes match immutable 59a4980. The sets differ only in the three runtime files and one test file below. The ordinary check receipt's 323 input hashes all match the candidate freeze.

| Source relative to `crates/dockspace/src/` | Actual control SHA-256 | Measured candidate SHA-256 |
| --- | --- | --- |
| `layout.rs` | `02a94c4c811b47214f0ca930f69b68bb94d18f25ec9c9d28644102536ad38f21` | `478cf5a1f3c6cf41cd3f21f3dfdc301a350957f18f58f900376edb256e38c05e` |
| `layout/projection.rs` | `d82120c1f004511128e65ad7a57f907583c150545ff286acfead2bc4cbfeeed4` | `1a54741ad89e01e2011a35d1f60c4896f2af7fe5b375d4df4d105318b8b4cfa9` |
| `layout/topology.rs` | `d2d06077b9a5bfb03f764b0ed9c862de4e9aca3a65946059e480b8412f356f56` | `1dc1264af676b5a083a299332203057b02004d174c8d4e27e45c6074f513a26c` |
| `layout/topology_tests.rs` | `e7bacde83b6d7c957ee8d749cdaf70fb06ca6f85c5850515169e2bd279b1d729` | `31c5e094b6c48cd58922de2642a08e223d1f5606f00463d1387e94a08ff93a2b` |

The immutable core delta is exactly runtime +131/-69 across three files and tests +139/-0. `layout/reference.rs` is unchanged at `2b2eee151d88bdd83048f0dfe7a4931a769aff6616da59e060200819008b209a`. The application receipt, original manifest/READY, and all five complete reviewed package pairs agree with these commits. Independently applying each of the frozen runtime patch and original primary patch in memory reconstructs the measured tree in twenty hunks; reversing either reconstructs the exact control. The archived eleven-hunk `parent.patch` also reconstructs the actual 59a4980 parent from immutable e3, in both directions. This is the retained frame/binary/gather parent, without rejected slab, childrows, or later axis validation changes.

The source-only historical reviews remain correctly separated. The first rejected report (`f3e7d6a8d8bb3ba4c9edad4d9751d2c74e20dd558796ef57511e1c6bf632bbbb`) identifies the debug-only first-visit guard blocker. The scalar-parent report (`abf9e65ee6384ae637373d980419d1a52883e36edca20d1d589e6bb075c98f31`) checks its repair on an earlier parent; the retained-parent report (`9a0dc619a90cd445c135e60784938034c46d1a499efee8bdfc6d366bf3b90b47`) binds the actual retained composition. All three archived copies equal their original reports. These are source review history, not additional executed candidates; the archived application, ordinary checks, build, and timing receipts identify the final primary patch and source set.

I checked the actual measured `layout.rs`, not just the review's claim: `visited_prefix` is initialized in every build at line 966, and every Enter performs ordinary `assert!(row <= visited_prefix)` at line 970 before preparation or rect mutation. Equality advances the prefix; prior rows remain legal raw revisits. Only final completeness at line 995 retains the debug condition. The measured projection constructor therefore does not substitute prefilled length for release traversal rejection. The original checked `solve_axis_into` remains in these measured bytes; there is no `CHECK_AXIS_INPUTS` overlay.

The preserved successful production build names one dockspace test executable. Its receipt records unchanged inputs and the separate timing counter bypass. I read and hashed the frozen executable bytes without running them:

| Executable | SHA-256 |
| --- | --- |
| Unified candidate | `270bbd69fd9856dddb8ea22ba0a9ed7edfa74a0b3e09a0f3101dac89f9a9300f` |
| Actual binary-input control | `4a41d0c57fcc97a256638919163c6d1d70f09473c3c6a56b8855ead5edfe4cf8` |
| Pinned ImGui | `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0` |

Both environment receipts bind those executables, the exact candidate source set, actual 59a4980 control or intended ImGui pin `d88f238d1d363ef387382b2e29ad12cf96f826ac`, result hashes, and unchanged frozen inputs/executables at finish. The archive preserves source/log evidence and binary identities; the executable files themselves remain external frozen artifacts, not archive members.

All twelve ordinary checks have zero exits, sequential receipt intervals, matching logs, and matching source inputs. The logs report 431 focused tests, four work regressions, 2,072 production core tests, four public integrations, 2,248 workspace tests, 292 default host tests, and 335 serde host tests. Formatting, correctness/suspicious Clippy, strict docs, ten doctests, and dependency boundaries also pass. Full core/workspace retain the same three opt-in skips; focused filter exclusions are not extra executed coverage. Both new central/raw-DAG witnesses pass in the focused, production, and workspace logs. Read-only inspection of archived verification source confirms removal of the timing counter bypass for ordinary checks/work and `RUSTDOCFLAGS=-D warnings` for docs.

The separate ordinary work probe has a zero-exit receipt and passing log before the timing freeze. Its complete 21-row, 20-column CSV equals both the original binary-input control CSV and that control's independent archive byte for byte. All expected cell keys are unique and present. Edge cells alone have one successful root-body completion each, totaling six bodies and 2,926 rows. These are logical-work counts, not allocator calls, allocation bytes, peak residency, or stack sizes.

Each comparison contains serial ABBA then BAAB groups, two processes per condition/group, 48 measured samples per process/cell, and eight declared warmups. All sixteen process receipts have zero exits and non-overlapping intervals. I checked every raw row for its unique sample/cell key, intended product, complete node/leaf/split/item/root counts, alternating resize width, fixed height, nonnegative phases, and exact phase sum. Per-process CSVs reproduce aggregate rows exactly. Every condition/group/cell has 96 observations, giving 8,064 rows per comparison and 16,128 total.

All 336 complete vector files parse with unique root/path identities, expected binary depth-first structure, complete state counts, contained bounds, valid selected membership, and exact ownership of every expected item. Complete bytes agree for every workload cell across products, conditions, groups, and processes. I independently recomputed little-endian u64 FNV digests and matched each dump's sample-zero raw digest. All raw digests agree by workload and actual input width, including the alternate resize input. This checks full vectors rather than only their equality flag.

Nearest-rank recomputation, `ceil(n*p/100)-1`, reproduces all five phases' p50/p95/max in all 168 summary rows, every ratio/reduction in all 84 complete-operation comparisons, and the complete decision statistics:

| Statistic against actual 59a4980 control | Improve / regress / tie | Reduction range |
| --- | --- | --- |
| p50 | 39 / 2 / 1 | -2.88066% to +10.27692% |
| p95 | 31 / 10 / 1 | -19.26753% to +22.82807% |
| max | 25 / 16 / 1 | -339.61548% to +88.38445% |

The p50 non-improvements remain explicit:

| Group | Panes | Views | Case | Control / candidate ns | Reduction |
| --- | --- | --- | --- | --- | --- |
| 1 | 16 | 1 | center_move | 2,500 / 2,500 | tie |
| 1 | 1024 | 4 | cross_view_move | 86,041 / 86,167 | -0.14644% |
| 2 | 16 | 1 | resize | 1,458 / 1,500 | -2.88066% |

The worst p95 and max cells are both group 1, 128 panes, one view, edge_split: p95 19,250 -> 22,959 ns (-19.26753%), max 23,458 -> 103,125 ns (-339.61548%). P95's tie is group 1, 16 panes, four views, center_move; max's tie is group 2, 16 panes, four views, resize. These tails remain adverse observations. The records do not identify their exclusive cause or make maxima reproducible bounds.

Against ImGui, p50 improves in two cells and trails in forty; p95 improves in four and trails in thirty-eight; max improves in eleven and trails in thirty-one. All comparison rows remain present. The two p50 wins are 1024 panes, four views, edge_split, with reductions of 10.48476% and 8.85266% in groups 1 and 2.

| Panes | Complete p50 candidate / ImGui, both groups | At most 2x |
| --- | --- | --- |
| 16 | 2.06456x to 3.94144x | 0/14 |
| 128 | 1.26900x to 1.72844x | 14/14 |
| 1024 | 0.89515x to 1.20462x | 14/14 |

The total is exactly 28/42. Every small cell still exceeds 2x, so the target remains active and incomplete. The immutable plan and document agree; the earlier commentary correction does not change the archived statistics.

Immutable timing source still includes request capture, checked mutation, fresh all-root leaf inputs/layout, and fresh complete owned export in the complete interval. Fixture creation, independent validation/digest/I/O, and final disposal of rich outputs remain outside it. The fixture has binary logical trees, no floating roots, and no designated central leaf. Therefore this campaign does not separately measure a central-path speedup, the contained minimum-only discard path, physical native settlement, or all possible topology shapes. The semantic gate witnesses cover broader behavior without becoming performance measurements.

The negative costs are correctly retained. Every measured row initializes a rectangle placeholder before projection overwrites it. The contained scene caller still measures minima before clipping and can discard the measurement without projecting; that path pays rect initialization/residency without receiving a later output-buffer transfer saving. Original child gather Vecs/copies and their wrapper field remain, record stride changes, the scalar frontier and release prefix perform work, and split outputs retain three extent arrays. Timings establish no exclusive allocator attribution, allocation-byte count, ABI/stride size, peak residency, or architectural lower bound.

Retention is limited to this exact three-runtime-file variant on the actual binary-input control, supported by this campaign's complete p50 distribution while accepting the visible negative cells and tails. It neither restores rejected slab/childrows nor adopts later axis or ownership changes. Source reviews are not counted as executed draft variants, and the local checks are not fresh remote CI. New runtime heads still require their own qualification, measurement, and remote evidence.

Only `/tmp/unified-values-evidence-review.md` was written. No Cargo, compiler, formatting, tests, timing, benchmark, runner, archived script, network, or child agent was executed. Repository, fork, cache, candidates, and existing evidence were not modified. Read-only immutable Git, archived data, frozen binary bytes, and independent arithmetic supplied this review.
