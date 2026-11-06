# Independent evidence review: NodeResult checkpoint

## Findings

**No blocking evidence finding.** The immutable checkpoint, exact archived source pairs, qualification receipts, original observations, complete vectors and published comparisons agree. The report correctly retains NodeResult for its simpler single result owner and public interface, while explicitly declining to claim a stable hot-path speedup. It includes the material regressions and leaves the twofold ImGui goal active.

The current live MRU edits are outside this review. Candidate identity is commit `fa1d4d0e4e1e81ef0641d6e682a8de870aac53ec`, not the current worktree. The actual measured control is commit `0b8ab8e2ab7908af9fce477b320dff221e2aae92`, not a later documentation/native-CI commit. No candidate drift was inferred from later live edits.

Completed at `2026-11-06T22:02:23.573242+00:00`. This was a bounded evidence audit; I did not run Cargo, compilation, tests, benchmarks, timers, any archived runner, Git writes, network access or subagents. I wrote only this report.

## Artifact and immutable-source binding

The requested report and evidence files are byte-identical to their versions in checkpoint `fa1d4d0e4e1e81ef0641d6e682a8de870aac53ec`:

| Artifact | SHA-256 |
| --- | --- |
| `docs/performance/2026-11-07-node-result.md` | `525cfedcc17d2fcecaccdcf798d267d7b41f32ddca23d8ddd8c9b4ac57384175` |
| `evidence.tar.xz`, 430,176 bytes | `8a42ffeac64fe81040cbb818743d8ad956abde6e23741e90d8cfaa1bdbe4d077` |
| `comparisons.csv` | `61f048d156ad0b50b39dd2e0c208db8922af448cc26fb561bd39166600eecd7b` |
| `checks.json` | `fe0fe862a57d375053414b3dd5c17429c82e88c1e2b9c60b32ccb5433446efb2` |
| `archive-check.json` | `4a65d0b536c469a231f442a013d5b2cee98cd442f6cf42560dd76c8aed8415f2` |
| Archived `manifest.json` | `c4591a854caf5d36e96e9dca3e1689c2c75f663e694aca26a3e6fa6d625faaa2` |

The archive has exactly 455 manifested regular inputs plus its manifest, with no duplicate members, links, absolute paths, parent traversal or unmanifested inputs. Every archived input was read and independently matched to its manifest byte length and SHA-256. The packaging script also explicitly reads every member back and compares its full bytes with the original in-memory input. The 411 archived files under the requested `node-result-frozen/` and `node-result-verified/` roots independently match their current temporary copies byte for byte. Each of the thirteen archived control/candidate pairs was compared with its corresponding immutable Git blob.

All 324 source hashes in the formal candidate receipt match checkpoint `fa1d4d0`; all 324 hashes in the frozen control receipt match `0b8ab8e`. The 323 counters-enabled qualification source hashes match the candidate receipt exactly; the one additional formal-receipt path is the ImGui comparison C++ source. This binds the Rust source/configuration and wrapper used by the receipts without consulting live candidate files.

For reproducibility, source-set hashes below encode ascending UTF-8 path + NUL + lowercase hexadecimal SHA-256 + newline:

- Formal candidate 324-source set: `e965f629fae17213db96ebd372ecefaa73f298f30e55f12d07c2286f77ea5aa5`.
- Frozen control 324-source set: `e0e1542becf92fa7277175a9ca48785d8a1e505a235767f659e12e292fb07948`.
- Exact thirteen changed control sources: `982203001c6fe798357197fc1573ceab3b53bf01cb7b1ff37d964d2cbaa39863`.
- Exact thirteen changed candidate sources: `eacc3f1c947a5d8a7159c5b6903265893f1b412c90f9fd61cbd85a81c0600737`.

The original reviewed patch is SHA-256 `274634d11c456f95515b8428917eb5f9bd439ec9ae96493165ce4408d1a86232`. I reconstructed its thirteen outputs from its captured-base identities and all 63 original hunks in memory; every reconstructed output matches the original candidate hash. Twelve measured outputs are exactly those reviewed outputs. The remaining `docking_cost.rs` difference is the existing actual-control work-counter overlay: old reviewed base -> actual control and reviewed output -> measured output have exactly the same three removed/inserted blocks. They add the three RootStructure CSV fields, format slots and arguments. No semantic NodeResult hunk was dropped.

The frozen `runtime.patch`, SHA-256 `98f2e8b26abff86d92f53cf7e5b307effc2f1885a3ddf7ca63875be9dd27439f`, exactly equals the immutable control-to-checkpoint diff scoped to `crates/dockspace`. That diff contains exactly these thirteen sources:

| Checkpoint candidate source | SHA-256 |
| --- | --- |
| `crates/dockspace/src/docking_cost.rs` | `3043546e6fa33ffe73511a1035d0ed4366f8a076e340241448f79073db33a00d` |
| `crates/dockspace/src/drop_resolver/preview.rs` | `7424c191195ea57da47991f9d4e3e937d9e75dd2519a1cfbd361809f3ba3e827` |
| `crates/dockspace/src/engine/splitter_geometry.rs` | `2019f0ad456794ae23b4c4aabeb14f7e7554d17104d7ceb8a5b0e88e90ba4f36` |
| `crates/dockspace/src/engine/splitter_geometry/tests.rs` | `735d87c0b79e887b873006a9b12df17566ec6fe55897ad24f59fc345b7cd96b0` |
| `crates/dockspace/src/engine/surface_contribution/reuse_tests.rs` | `6021006a324e79d6a25157bd028626ea8aa10f179b13f38107e3e659b0cdad95` |
| `crates/dockspace/src/layout.rs` | `325f56f16cacbde46cce531461c8830417a6aa8ff4af06f6c87145a7528963df` |
| `crates/dockspace/src/layout/layout_cost.rs` | `420140d7cca7915966e0103c06ad27453d8b8fddba5f13a090dcab0d9691250e` |
| `crates/dockspace/src/layout/projection.rs` | `d86c008dd78c5f1ed805de76b71d102a29dd3dab4e6ee37ca8f3cb74839edfb0` |
| `crates/dockspace/src/layout/reference.rs` | `2b2eee151d88bdd83048f0dfe7a4931a769aff6616da59e060200819008b209a` |
| `crates/dockspace/src/layout/topology_tests.rs` | `5863123fd6b1733f1ad1e0bfeb0d9f67b6c18e9b20703b9705079e3ae90cf5db` |
| `crates/dockspace/src/scene_compiler/future_layout.rs` | `bb0faa4b98574a70e5fc38a28c391e8970a40aa24942bdc07b5980e68ec9eba9` |
| `crates/dockspace/src/scene_compiler/surface.rs` | `6f3d945673d9032cfc9eb629bf98e57da3c525318a77ddd08c7657dd11aa5b2c` |
| `crates/dockspace/tests/layout_solver.rs` | `4b750cd3f85457f26a86529011b5e8396ea04a0dcd46648180969d55943014ee` |

The temporary NodeResult binary was independently hashed as `140b8162ee98e5ef24df6062ac264acc62848a26bc0b2f5d9dc41fab31da4d5c`, matching its formal build receipt and both run environments. The RootStructure control receipt and environment bind binary `d0facb66482a23377bcdaad5e3bb5be04de9fe6ee35f521a51d78e24cfdf9ba2`. The ImGui environment identifies pinned commit `d88f238d1d363ef387382b2e29ad12cf96f826ac` and binary `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0`. These latter binary identities are receipt evidence; this review did not execute or rebuild them.

## Qualification and complete-operation boundary

All twelve counters-enabled qualification receipts report return code zero, preserve their source hashes and match the published `checks.json`. Archived logs substantiate 2,065 production core cases, 2,241 workspace cases, four no-default public integrations, 292 default host cases and 335 serde host cases. Ordinary production/workspace runs each skip the three opt-in probes. The focused selection has 409 passes, the separate work regressions have four passes, and doctests report 9 core plus 1 adapter pass. Formatting, correctness/suspicious Clippy, strict docs and dependency receipts are successful. These are verified existing receipts, not new execution by this reviewer.

The independent ordinary work probe has all 21 original cells. Its RootStructure completion fields are exactly `(0,0,0)` for resize, center and cross-view move; edge split is `(1, 2*(panes/views)+1, panes/views)` for completed bodies, rows and split handles. The complete counters/runner suffix remains byte-identical to the actual control: 3,554 bytes, SHA-256 `c353cbe1d47dee5d118fa914a282e5d02a282f3d83120ed35db4039f40e33134`. This checkpoint still has the independent MRU store and its counters; later MRU work is not mixed into these observations.

The following actual-control/candidate source regions are independently byte-identical:

| Region | Bytes | SHA-256 |
| --- | ---: | --- |
| Fixture/request/fresh layout prefix, file start to `fn append_node` | 8,363 | `1567481d1ca73d8618ecea1a4699c3f45804fd2e566633a40d6488d02e39b58e` |
| Full `fn measure(` through before `fn split_rect(` | 1,123 | `a056aff86457f1b38c7850085d0fa74b736b0ebc06195d05a3cc6f58d7dd1011` |
| Formal docking-cost test attributes/function through before the work-test attributes | 3,767 | `270a71b651925b0a28ba89b3a6a57f01571efed4d2d98c7da9d4070d045b3126` |
| Expected complete-word oracle, `fn split_rect(` through before `struct Counts {` | 3,249 | `f23912b81b295028e311c6f000d0423c8037f0934879f0a32b3114e2edc7cb68` |

All ten source regions named by the original contract receipt remain byte-identical between the actual immutable control and measured candidate. This includes numerical solver/geometry logic, overrides, constraint traversal, map query/iteration, complete split output and independent reference implementation. The reference implementation itself retains SHA-256 `a94d7bafd8d07c4bcfeb934a36c86676baed83e54e4eaf944a7b01bc932b22b3`; its actual-output capture adapter alone migrates to NodeResult.

The formal docking source changes only NodeResult access/iteration and its corresponding single-map output-inventory check. Request capture, checked mutation, all-root fresh leaf constraints/layout and fresh complete owned export stay inside the same complete-operation timer. Export adapts preorder NodeResult rows to rectangle references without reducing output words. The measured operation is read directly from `operation_owned_ns`, not synthesized from phase medians. Every raw row also satisfies request + mutation + layout + export = complete-operation duration. Retained projections and words remain owned across fixture disposal.

## Original observations, complete vectors and all comparisons

Both independent pairings use ABBA then BAAB, eight process runs each, 48 recorded samples after eight warmups per process/cell, the exact original 21 cells, and 96 observations per condition/group/cell. All sixteen recorded process exits are successful. The RootStructure pairing precedes the ImGui pairing; each pairing's candidate source set and binary identity match the same formal receipt.

Independent readback found:

- 8,064 raw rows per pairing, 16,128 total. Each aggregate exactly reproduces its eight original process CSVs. Every process has the exact 21 cells and sample IDs 0..47 once per cell; widths, products and node/leaf/split/item/root inventories match the protocol.
- 336 complete vectors, exactly 21 per process. After matching the product-specific filename prefix, each cell's complete vector bytes agree across all sixteen runs and both products. All words were read; each vector has `9*nodes + items` words and its independently recomputed full-word FNV digest equals that process's sample-zero digest. All 27 raw state identities, including both resize widths, also have consistent recorded digests across runs.
- All 2,520 published phase summary values were recomputed from original integer observations using the established nearest-rank definition: rank 48 for p50, rank 92 for p95, and rank 96 for max among 96 values.
- All 84 comparison rows, including every p50/p95/max baseline value, candidate value, ratio and reduction, were independently recomputed and agree with `comparisons.csv`. No phase, tail, losing case or comparison group was discarded.

Negative reduction means candidate regression. The complete-operation findings are:

| Reference | Statistic | Reduction range | Negative group/cells | Ties |
| --- | --- | --- | ---: | ---: |
| RootStructure | p50 | -11.774850% to +10.546258% | 22 / 42 | 1 |
| RootStructure | p95 | -166.098597% to +14.286638% | 19 / 42 | 0 |
| RootStructure | max | -728.991304% to +82.213439% | 23 / 42 | 0 |
| ImGui | p50 | -433.280000% to -16.543039% | 42 / 42 | 0 |
| ImGui | p95 | -618.290855% to -0.304994% | 42 / 42 | 0 |
| ImGui | max | -4988.495575% to +81.301796% | 31 / 42 | 0 |

For an explicit audit trail of every RootStructure loss and tail, all 42 group/cell comparisons follow. These use the already-published comparison data, not a new statistic or measurement:

| Group | Panes | Views | Case | p50 reduction | p95 reduction | Max reduction |
| ---: | ---: | ---: | --- | ---: | ---: | ---: |
| 1 | 16 | 1 | `resize` | +2.584615% | -15.520000% | -170.666115% |
| 1 | 16 | 1 | `center_move` | +0.035286% | -1.028655% | -162.754272% |
| 1 | 16 | 1 | `edge_split` | +0.021815% | +2.255910% | +1.895735% |
| 1 | 16 | 4 | `resize` | +0.051046% | +1.938163% | -1.692247% |
| 1 | 16 | 4 | `center_move` | -2.587282% | -1.171429% | -4.309677% |
| 1 | 16 | 4 | `edge_split` | -2.857143% | -17.172853% | -728.991304% |
| 1 | 16 | 4 | `cross_view_move` | +0.000000% | +13.990881% | +76.209744% |
| 1 | 128 | 1 | `resize` | +0.763218% | -2.484946% | -302.919661% |
| 1 | 128 | 1 | `center_move` | -8.411215% | +5.874680% | +47.973481% |
| 1 | 128 | 1 | `edge_split` | -3.209639% | +14.286638% | +24.301500% |
| 1 | 128 | 4 | `resize` | +2.834043% | -2.636981% | -284.367115% |
| 1 | 128 | 4 | `center_move` | +5.553333% | -6.540177% | +68.204612% |
| 1 | 128 | 4 | `edge_split` | +1.392137% | +10.201265% | -39.922759% |
| 1 | 128 | 4 | `cross_view_move` | +3.633438% | +13.629411% | +62.362223% |
| 1 | 1024 | 1 | `resize` | -1.730783% | +8.802817% | +13.459517% |
| 1 | 1024 | 1 | `center_move` | -0.234443% | +2.613277% | -3.220768% |
| 1 | 1024 | 1 | `edge_split` | -0.601352% | -3.088079% | +7.612155% |
| 1 | 1024 | 4 | `resize` | -2.266369% | -12.420629% | -49.115696% |
| 1 | 1024 | 4 | `center_move` | -3.003970% | -6.460937% | -46.207073% |
| 1 | 1024 | 4 | `edge_split` | -0.300101% | +2.566090% | -27.193117% |
| 1 | 1024 | 4 | `cross_view_move` | -1.764009% | +10.266636% | -0.636145% |
| 2 | 16 | 1 | `resize` | +2.523077% | +2.400000% | +21.640000% |
| 2 | 16 | 1 | `center_move` | +10.546258% | +1.028404% | +82.213439% |
| 2 | 16 | 1 | `edge_split` | +5.128205% | -26.832018% | +20.662044% |
| 2 | 16 | 4 | `resize` | +8.178257% | +7.286213% | +8.570449% |
| 2 | 16 | 4 | `center_move` | +9.099408% | +8.147321% | +35.749483% |
| 2 | 16 | 4 | `edge_split` | +3.759058% | -3.083456% | +10.096931% |
| 2 | 16 | 4 | `cross_view_move` | -2.587282% | -17.786667% | -675.584307% |
| 2 | 128 | 1 | `resize` | -3.781818% | +11.375395% | -105.944295% |
| 2 | 128 | 1 | `center_move` | -7.647706% | +10.903219% | -3.541117% |
| 2 | 128 | 1 | `edge_split` | -11.774850% | -45.551309% | -242.087784% |
| 2 | 128 | 4 | `resize` | -1.048922% | +3.179699% | +68.678732% |
| 2 | 128 | 4 | `center_move` | -5.856398% | -166.098597% | -122.298476% |
| 2 | 128 | 4 | `edge_split` | +0.901390% | -8.292484% | -159.769026% |
| 2 | 128 | 4 | `cross_view_move` | -4.882759% | +5.271942% | +68.869466% |
| 2 | 1024 | 1 | `resize` | +0.380933% | +6.851190% | -45.446508% |
| 2 | 1024 | 1 | `center_move` | +1.736536% | +2.009148% | +23.476367% |
| 2 | 1024 | 1 | `edge_split` | -0.349178% | +6.652008% | -30.974843% |
| 2 | 1024 | 4 | `resize` | -1.511201% | -123.713702% | -2.725777% |
| 2 | 1024 | 4 | `center_move` | -0.774014% | -0.068856% | -37.794137% |
| 2 | 1024 | 4 | `edge_split` | +0.432158% | +10.258795% | +0.131734% |
| 2 | 1024 | 4 | `cross_view_move` | -0.581395% | -4.241782% | +40.575565% |

The 128-pane one-view center case regresses **8.411215% in group 1 and 7.647706% in group 2**. The same size/view edge case regresses 3.209639% and **11.774850%**. At 1024 panes, p50 changes span -3.003970% to +1.736536%; these are small mixed changes rather than evidence of a stable hot winner. The evidence does not supply a new statistical significance analysis, and the report does not pretend that it does.

The independently reproduced complete-operation p50/ImGui table is:

| Total panes | Both groups' ratios | Group/cells at most 2x |
| ---: | --- | ---: |
| 16 | 2.467200x to 5.332800x | 0 / 14 |
| 128 | 1.497325x to 2.307137x | 7 / 14 |
| 1024 | 1.165430x to 1.609839x | 14 / 14 |
| Total | All 42 group/cells retained | 21 / 42 |

The remaining 42 ImGui comparisons, including every tail loss, remain fully present and verified in the bound `comparisons.csv`. All p50 and p95 group/cells are slower than ImGui; 31 max ratios are slower. The table and published narrative use the complete operation, retain all negatives, and correctly leave the twofold goal unachieved.

## Review boundary and handoff

The evidence supports a source/interface simplification and a fully disclosed mixed performance result. It does not support a stable speedup, an allocator-byte claim or closure of the twofold objective. The report already states these limits and discloses the material 128-pane regressions, small 1024-pane changes and extreme tail regressions. No report correction or extra statistical/validation framework is required by this review.

Only the immutable NodeResult checkpoint and its frozen/archived receipts were assessed. Later MRU code is neither a changed candidate nor evidence contamination. This audit does not replace the earlier independent full source review and does not re-run qualification. Review complete; STOP.
