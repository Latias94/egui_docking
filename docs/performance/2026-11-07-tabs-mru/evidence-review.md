# Independent evidence review: Tabs-owned MRU

## Findings

**No blocking evidence finding.** The exact 24-path reviewed change, immutable candidate/control commits, qualification receipts, original observations, complete output vectors, work accounting and all published comparisons agree. The report includes all four p50 regressions and all tail losses, correctly retains the twofold goal, and does not turn unsuccessful type-size diagnostics into size/allocation evidence.

The candidate is immutable commit `865409041a33cfb13442d5ea16f2035cf5feff0a`; its actual control is `fa1d4d0e4e1e81ef0641d6e682a8de870aac53ec`. Later placement-sharing work is outside this evidence and was not treated as candidate drift. This review does not authorize mixing those unmeasured sources into the MRU result.

Completed at `2026-11-06T22:23:10.176680+00:00`. I ran only read-only artifact/source/data checks. I did not run Cargo, compilation, tests, benchmarks, timers, archived runner scripts, Git writes, network operations or subagents. The only written artifact for this evidence review is this report.

## Exact stable package and source binding

The report and its complete evidence directory match their immutable `8654090` versions. The final package was reread after the diagnostic-only repack: it contains **483 manifested input files plus manifest.json**, 518,864 compressed bytes. The previously observed report count was updated to 483; final archive/receipt/checksum bytes stayed stable across verification.

| Artifact | SHA-256 |
| --- | --- |
| `docs/performance/2026-11-07-tabs-mru.md` | `3e7be12ab4d90f63c3190974c28fe6601df91473eff2ad9d4da2fd5cadc08350` |
| `evidence.tar.xz` | `bc29b643046c17073caf3d47d93aa504944a8ba92af1c29f1cb14172540bdc64` |
| `comparisons.csv` | `2c0600a29ed6c276e733b4c9c59704d53a346b067f36bb78bcdf4c0b1332c5b6` |
| `checks.json` | `ae343c9a7b629500c1f6f409b86cfacca8aa55375fb75310dd934cb5581fe059` |
| `archive-check.json` | `a8e5a7a10364532da8fa19fbf7a4c48633dfdbf55a794f70bae4774f89c14fe8` |
| Archived manifest | `c500caa674c8235fddbc8d669eb61f0c7d6527ea0ae845addd233b55f86b0cc7` |

Every regular archived input was independently read and matched to its manifest SHA-256 and byte length. There are no duplicate, missing, extra, linked, absolute or parent-traversing input members. All published SHA256SUMS entries match, including the preserved original independent source review. The packager itself retains explicit full-byte readback comparison of every input. All 416 archived frozen/verified files match the specified temporary originals byte for byte.

The immutable source checks establish:

- All 324 formal candidate source/config/wrapper hashes match `8654090` Git blobs.
- All 324 actual-control receipt hashes match `fa1d4d0` Git blobs.
- All 323 counters-enabled qualification source hashes match the formal candidate receipt; the additional formal-only key is the ImGui comparison C++ wrapper.
- Exactly 24 core source paths differ between those commits. Every archived control/candidate pair matches the corresponding immutable Git blob and the reviewed rebase manifest's complete base/output hash.
- Reconstructing all unified diffs from those 24 pairs reproduces the reviewed patch bytes exactly at SHA-256 `9046e9d14bb92bdb7aab2e8935b08aa3f4f5849f135e1b0edef595f50f216c01`.
- The original MRU patch remains archived at `1ea14af6f5cdcefea0bb170390dbad8270f3fd845c1cbe975a27d3830b13e9d8`; the earlier independent rebase review is preserved, rather than regenerated around later results.
- The formal runtime Git patch is exactly the immutable control-to-candidate core diff, SHA-256 `eeebe17829c70f3742f28b7cd3ada65208dae5657b3e8097319a316afbfbf5da`.

Source-set hashes use ascending UTF-8 path + NUL + lowercase hexadecimal SHA-256 + newline:

| Source set | SHA-256 |
| --- | --- |
| Formal candidate 324 sources | `ee86c9c24650537ff0a5d9c678580bb88b965bbc7bb59b91eee6a57faae3400c` |
| Actual control 324 sources | `e965f629fae17213db96ebd372ecefaa73f298f30e55f12d07c2286f77ea5aa5` |
| Reviewed 24 control files | `e44e1de5699e9c436f79d04fdda74e7916d24b286949ec776fbd538662898d28` |
| Reviewed 24 candidate files | `7dde1e11eb8b9468b51d15570ba4848ab768a1706b7c1840559c127a3ad2cebd` |

The candidate executable was independently hashed as `cba8437ec30e46186ec8c644b0474e8a97d4531ab34d0d727deab8f59728f31e`, matching the build receipt and both environments. Its control is binary `140b8162ee98e5ef24df6062ac264acc62848a26bc0b2f5d9dc41fab31da4d5c`. Pinned ImGui remains commit `d88f238d1d363ef387382b2e29ad12cf96f826ac`, binary `32fa4e38b04d24719c6b0116bc3b97488a5cfba6fa7184c2777e79036b8d86c0`. Neither receipt is inferred from the current worktree.

## Qualification, registered history witness and honest work counts

The twelve qualification entries exactly match published checks.json, report zero exits and preserve their source identities with counters enabled. Archived log summaries support 427 focused passes, four work-regression passes, 2,068 production core passes, 2,244 workspace passes, four no-default public integration passes and 292/335 default/serde host passes. Production/workspace runs skip the three opt-in probes. Formatting, correctness/suspicious Clippy, strict docs, doctests and dependency checks have successful receipts. These are inspected existing execution receipts, not new tests run by this reviewer.

The actual measured `owner_reference.rs` equals its complete prior module followed by the registered history-only test. The base hash is `99e0e96521bc91aab550fb66e281a4ac5cf2599060d11e6832b1fd6e23349768`; candidate hash is `8a39eb6df3d844f952af7b550f75f51c51ab6d5c853b47f8748b69adb55158d4`. The registered test executes actual-stage captures selecting `[3,1]` in a three-item stack. It checks changed outcomes/report, original visual order and selected item, new MRU `[1,3,2]`, old Workspace MRU `[1,2,3]`, Workspace inequality, stale old source reference and actual placement. No previous test byte is deleted or weakened.

The ordinary probe has exactly the original 21 cells and 20 columns. It removes only the detached-store counters `mru_store_forks`, `mru_handle_clones` and `mru_payload_clones`; actual history elements remain recorded as `mru_item_copies`. Independent CSV checks reproduce:

| Operation | Actual MRU elements copied per cell | Completed bodies / rows / split handles |
| --- | ---: | --- |
| Resize | 0 | 0 / 0 / 0 |
| Center move | 4 | 0 / 0 / 0 |
| Cross-view move | 4 | 0 / 0 / 0 |
| Edge split | 2 | 1 / `2*(panes/views)+1` / `panes/views` |

The source records this at the concrete Node::clone hook, using `mru.len()` for each actual complete Tabs payload clone. The Clone implementation copies both items and history; the logical inventory still includes owned MRU lengths. The values are not fabricated zeros because a separate map disappeared, and they are not allocator-byte or timing measurements. The expected RootStructure completion counts remain unchanged.

## Complete measurement boundary and output evidence

The immutable baseline/candidate comparisons independently preserve these exact source regions:

| Region in docking_cost.rs | Bytes | SHA-256 |
| --- | ---: | --- |
| Fixture/request/fresh layout, `fn opposite(` through before `fn append_node` | 6,634 | `106603457dda45ece7e7f687d9a0123f9224578f88ae9f9bf003675287b9e7dd` |
| Complete owned exporter, `fn export_state(` through before following test attribute | 704 | `19a6031b1ccaeddd3195b31d6d55b460212d91df79cbed0875eb10ba8c5fc2da` |
| Full phase measure, `fn measure(` through before `fn split_rect(` | 1,123 | `a056aff86457f1b38c7850085d0fa74b736b0ebc06195d05a3cc6f58d7dd1011` |
| Formal docking-cost attributes/function through before work-test attributes | 3,767 | `270a71b651925b0a28ba89b3a6a57f01571efed4d2d98c7da9d4070d045b3126` |
| Expected complete-word oracle | 3,249 | `f23912b81b295028e311c6f000d0423c8037f0934879f0a32b3114e2edc7cb68` |

Tabs read-pattern adaptations do not narrow the exporter or keyed oracle. Request capture, checked mutation, fresh constraints for every leaf of every root, full projection and fresh complete owned export remain inside their existing boundaries. Numeric/layout sources outside the recorded 24-path migration retain the bound control implementation. Phase medians are not added to manufacture an operation duration; the comparison uses actual operation_owned_ns.

Each pairing independently runs ABBA and BAAB, eight successful processes, 48 recorded samples after eight warmups per process/cell, all 21 original cells, and 96 observations per condition/group/cell. Independent readback verifies:

- 8,064 raw rows per pairing, **16,128 total**. Each aggregate exactly reproduces its eight original process CSVs. Every process has every cell and each sample 0..47 exactly once. Product, resize width, height and complete structural/item inventories match the protocol.
- Every raw row's operation_owned_ns equals the sum of its four contiguous raw phase durations, retaining the complete timer identity.
- **336 complete vectors**, 21 per process. Every word was read. Complete bytes agree by cell across all sixteen processes and products; each vector contains `9*nodes + items` words. Recomputed full-word FNV digests match each run's sample-zero record, and all 27 recorded state/width identities have consistent digests across raw observations.
- All **2,520 phase summary statistics** reproduce the established nearest-rank p50/p95/max over 96 observations.
- All **84 complete comparison rows**, including every baseline/candidate statistic, ratio and reduction for p50/p95/max, independently reproduce from original raw observations.

No original cell, losing group, tail or output word was dropped to improve the result.

## Complete-operation outcomes and retained negatives

| Reference | Statistic | Reduction range | Improved | Negative | Ties |
| --- | --- | --- | ---: | ---: | ---: |
| Single NodeResult | p50 | -5.504183% to +13.378220% | 38 / 42 | 4 / 42 | 0 |
| Single NodeResult | p95 | -196.524330% to +84.431138% | 32 / 42 | 10 / 42 | 0 |
| Single NodeResult | max | -299.099984% to +94.953271% | 26 / 42 | 16 / 42 | 0 |
| ImGui | p50 | -413.698630% to -8.774253% | 0 / 42 | 42 / 42 | 0 |
| ImGui | p95 | -410.353535% to -2.685691% | 0 / 42 | 42 / 42 | 0 |
| ImGui | max | -2525.000000% to +76.666667% | 11 / 42 | 31 / 42 | 0 |

The four control p50 regressions are exactly those disclosed in the report:

| Group | Panes | Views | Case | Reduction |
| ---: | ---: | ---: | --- | ---: |
| 1 | 16 | 1 | Resize | -5.107692% |
| 1 | 128 | 1 | Resize | -1.140719% |
| 2 | 16 | 1 | Resize | -2.459016% |
| 2 | 16 | 1 | Edge split | -5.504183% |

All center/cross-view moves improve against the control in both groups at every pane count. At 1024 panes, moves span +4.119728% to +9.379487%. This is observed complete-operation evidence consistent with reduced ownership cost, not isolated attribution of an allocator share or a proof of uniform improvement. The p95/max losses remain material and fully preserved.

| Total panes | Complete p50 / ImGui, both groups | At most 2x |
| ---: | --- | ---: |
| 16 | 2.734400x to 5.136986x | 0 / 14 |
| 128 | 1.541296x to 2.080489x | 11 / 14 |
| 1024 | 1.087743x to 1.497933x | 14 / 14 |
| Total | All original group/cells retained | **25 / 42** |

The published rounded table agrees. Every p50/p95 group/cell remains slower than ImGui and 31 max ratios remain slower. Small-pane performance is still the large gap; the goal is correctly left active. The evidence does not include a new significance analysis and the report does not claim one.

## Raw visibility and unsuccessful type-layout diagnostics

Node and NodeResult being declared pub inside implementation modules does not by itself make them external APIs. The immutable candidate/control lib.rs keeps `graph` private; layout is public only in the unit-test configuration and private in delivered non-test builds. The report's raw-implementation description and model/runtime external-boundary wording are therefore correct. The current NodeResult report was similarly corrected. Immutable historical `fa1d4d0` retains its original `refactor(layout)!` commit message; this review does not reinterpret or rewrite history.

The external size probe's archived build log reports E0603 for both private modules and explicitly says Node is not publicly re-exported. This is an unsuccessful auxiliary diagnostic, preserved separately from the twelve successful qualification gates. It yields no size/alignment result.

The archived dwarfdump query returns zero but its 111-byte stdout contains only the Mach-O file-format header, with no type DIE or byte-size information; stderr is empty. Thus it likewise yields no Node/NodeResult type layout, and does not support a comparison with historical layouts. The report explicitly says that these attempts establish no actual padding, allocation bytes or historical type size. The warning that a larger complete enum can affect allocation footprint remains a source-level possibility, not a measured cost or victory.

## Handoff

The final MRU package is internally consistent, source-bound and complete for its stated protocol. It supports the reported mixed whole-operation improvement and 25/42 twofold threshold, while retaining all negative observations and unresolved size/allocation questions. No evidence correction is required.

Later placement or metadata-sharing candidates require their own qualification and comparisons. This review does not rerun the prior independent source review and does not claim any unmeasured combination. Review complete; STOP.
