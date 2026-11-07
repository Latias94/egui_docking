# Post-main current diagnostic evidence review

Finding first: the source/executable binding, three successful workloads and retained raw evidence pass this independent review. One saved classification needs a narrow correction before describing the whole attribution as excluding fixture work: `16-4-resize` contains one falsely included layout sample. Its `timed_layout` entry should be **169 inclusive samples / 8 outermost roots**, rather than 170 / 9. The center and edge phase counts, all 222 saved memory paths, and the previously established pending-stack location remain supported. This is an attribution correction, not a runtime, workload or formal-score failure.

I independently read the saved receipts, scripts, source snapshots, raw CSV/xz, sample trees and immutable Git objects. No Cargo, tests, runner, sampling, timing or network operation was performed. A narrow static disassembly of the probe's fixture call was necessary to resolve the newly found classification ambiguity; I did not repeat the previous builder/solver disassembly. I did not read the other agents' current-round reports. Only this report was written.

**Binding and the single-line change.** Evidence is under `/tmp/dockspace-imgui-twofold-20261107-ea4161e/post-main-continuation/current-main-profile/`. The receipt binds source HEAD `c7508eec3ac44138305797691244ae9c180d7680` to retained runtime `0e441c8ddeb6f85b9b0e8237303b5def32e178ae`, rather than the older `49baba1` profile. I hashed all 324 formal source entries from both immutable commits: every entry matches the receipt and `projection-stack-reserve-frozen/source-receipt.json`.

The formal and diagnostic maps each have 324 entries and differ only in `crates/dockspace/src/docking_cost.rs`. The saved diagnostic probe is exactly the original probe with one assertion changed:

```text
value <= 10_000, "benchmark iteration setting exceeds 10000"
→ value <= 1_000_000, "diagnostic iteration setting exceeds 1000000"
```

I regenerated that textual difference in memory and matched `only-probe-change.patch`. The setting helper also serves warmup, but these runs retain warmup eight; no measurement body, workload, validation, output or counter configuration changed.

| Artifact | Independently checked SHA-256 |
| --- | --- |
| Current `receipt.json` | `ec94fad210c92a69b6d0d86060bcf40b592542f7a0857f81353a159fc13a0f01` |
| Formal source receipt | `03a4404a1fa9ed1f38a8aad04e3cef4a64e4f984b0d9ff7bb55d758f6ea262f6` |
| Formal executable | `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b` |
| Diagnostic executable | `35eeac8262ad7a4445a6bfb14f4f03de1d1672181338d42ef922eabd09ca2590` |
| Original probe | `01d8438fc79095aacae935170aff3ed7eb8b5e497aa368c9966889b789adf2d3` |
| Diagnostic probe | `93879195c419a282c7bca446336af0a3b33d1e4854603cd71a6c8df5cb0afbf7` |
| Only-probe patch | `c9f5a2f5f95c7b465a82714b272806e50ad7d0e483fe20ffebcd7f9c11d53c73` |
| Saved `attribution.json`, before correction | `7604545a1451db79c66664bedf2f5d8b7b7299cf4f7286042a73bfd509a94962` |
| `profile_current_main.py` | `a495fe4c51ba880a0c9f830934f12d09e236c4950c2d97964c8f7b0ff5c2d99e` |

The build receipt and captured Cargo JSON record a successful optimized test executable, default features, debug assertions disabled, and one emitted dockspace executable. The script builds serially with the existing work-counter bypass, freezes that executable, and restores the original probe before launching any sampled workload. Its `finally` block verifies that no concurrent probe edit would be overwritten, restores the probe, and checks all 324 sources.

Recorded restoration is `2026-11-07T10:25:10.432276+00:00`, preceding the first workload start at `10:25:10.432796`. Final recorded source and executable checks are true. Both saved executable hashes still match when independently read. Current live `operation.rs` differs because of the separately authorized source-nonempty candidate; immutable source matching avoids conflating it with this historical diagnostic. This review does not rerun historical restoration assertions.

**Three complete diagnostic workloads.** Each workload and sampler has recorded return code zero. Each stdout independently reports one passing `docking_cost` invocation; each stderr is empty. Sampler logs identify the receipt PIDs and successful four-second sampling. Each run requests 500,000 measured iterations plus eight warmups and the exact ignored probe test with one test thread.

I read all 1,500,000 CSV rows, checked contiguous sample indices, case/pane/view identity, nonnegative durations, telescoping duration identities, inventory, widths and stable per-width state digests. Every raw byte count and SHA-256 matches its receipt; each xz's hash matches and decompression reproduces the corresponding raw hash and length.

| Case | Rows | Observed inventory / digests | Raw SHA-256 |
| --- | --- | --- | --- |
| 16 panes, 4 views, resize | 500,000 | 28 nodes / 16 leaves / 12 splits / 32 items / 4 roots; 250,000 rows each at widths 16,380 and 16,444; digests `0358a9379f081139`, `1ac18ebff6f6893d` | `319cddba88b38e946ad6f61355b076af328e223aa6e6d52ef68fb2405726e9d4` |
| 16 panes, 1 view, center | 500,000 | 31 / 16 / 15 / 32 / 1; width 16,380; digest `068047cc466c7222` | `4cfd134619f43987f4a265743ea5d1a0494bd96f78c6ece8b0a828df7e72bd4e` |
| 16 panes, 1 view, edge | 500,000 | 33 / 17 / 16 / 32 / 1; width 16,380; digest `54302b714c824f3d` | `dbd3bbdc7ea3068b7c5b44f90d4c395ae794141782133359551584811c2f3380` |

The probe source retains fresh per-iteration fixture construction, real request capture and mutation, fresh root measurement/project-all-roots, owned rich projections/report, full common export and correctness assertions. `measure` ends its complete-operation timer after export (`original-probe.rs:441–468`). Fixture setup precedes it; post-measurement `validate`, workspace destruction and final rich-result destruction occur afterwards (`794–820`). Production checks and temporary destruction inside prepare/project remain inside those calls; they must not be excluded just because post-operation validation/destruction is outside.

**Independent ancestry reconstruction and correction.** I reconstructed the raw `sample` call trees from indentation, selected outermost named prepare/project subtrees under the worker, and excluded explicit fixture ancestry. This exactly reproduced the saved initial phase counts and all saved memory paths. Worker totals are 2,904 / 2,925 / 2,926; they include untimed work and are not denominators for an operation-cost claim.

The explicit-fixture filter correctly removes 182 / 182 / 135 inclusive project samples respectively. However, absence of a fixture frame is not sufficient evidence of timed execution. Resize raw sample line 981 has:

```text
docking_cost::docking_cost +1652
  1 project_all_roots +2360
```

The same `+1652` outer node contains explicit fixture calls. Static inspection conclusively identifies the caller:

```text
docking_cost symbol start: 0x10084c470
0x10084cae0: bl fixture::h15f1711b5584d9ed
0x10084cae4: ...       # return PC, offset 1652
```

The resize sample's `0x100bb0ae4` is this return PC after its load slide. Whether the absent intermediate frame results from unwind presentation or another mechanism need not be asserted: the proven fixture callsite makes the one sample ineligible for a timed-layout count.

| Case | Strict timed layout, inclusive / roots | Timed prepare, inclusive / roots |
| --- | --- | --- |
| 16/4 resize | **169 / 8**, corrected from 170 / 9 | 0 / 0 |
| 16/1 center | 142 / 6 | 100 / 7 |
| 16/1 edge | 177 / 8 | 218 / 10 |

All remaining selected layout subtrees have the actual probe `+2272` project call ancestry; prepare subtrees have `+1828`. Every one of the 26 / 62 / 134 saved memory paths independently matches a raw-tree node beneath those proven callsites, without fixture ancestry. The misclassified resize node has no saved memory path. The earlier location review's edge `complete_item_move_structure +1352` reserve subtree, including 30 inclusive observations, is unaffected.

The minimal repair is to exclude that known setup callsite, or positively admit the already identified timed callsite, and record 169 / 8. Do not discard raw evidence or rerun the workload to conceal the correction. Any changed attribution file needs its own new hash. This review leaves the original evidence untouched.

**Permitted conclusion.** These current-runtime samples support narrow source-location hypotheses, including the previously reviewed pending BuildFrame reserve. They are not complete-operation scores, exclusive allocator shares, allocation counts, division shares or latency lower bounds. Nested reserve/grow/realloc paths overlap; their counts cannot be summed. Inline or unresolved direct probe code and post-return output destruction remain unclassified. Three sampled small workloads do not cover every formal cell or prove optimization exhaustion.

There is no source, executable, raw-data or workload blocker. There is a **specific attribution/publication correction** to the resize total and the unqualified “excludes fixture” claim. Once that is made explicit, the remaining diagnostic findings can be retained without a new build or sampling run. This is not a stop/continue verdict or qualification of a later candidate.
