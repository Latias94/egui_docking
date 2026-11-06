# Retain surface facts across local selection

Date: 2026-11-06. Baseline: `34ab23adf8062960298abbc5fbcba370104f3b86` (runtime `5484071`). Status: qualified.

This implements the [retained-facts plan](../plans/2026-11-06-001-dockspace-retained-facts-refactor.md), following the [architecture diagnosis](2026-11-06-architecture-design-review.md).

The complete 1,024-pane two-frame click p50 decreases by **33.87% and 33.45%** in two serial groups. Complete renderer-free selection process p50s decrease from **9.56–9.75 ms to 1.79–1.80 ms**. Selection's tracked update allocations decrease by 77.46%, requested bytes by 58.79%, and end-of-update live bytes by 25.32%. This is a material reduction in unrelated derivation and immutable value comparison, without skipping current measurements, contents or output settlement.

## Architecture and deleted work

An ordinary Select previously changed one stack's selection/MRU but regenerated presentation requirements and the affected surface's complete scene. Geometry records also embedded current selection or command proofs, so identical layout could not outlive that edit.

`PresentationPlan` now binds an immutable `Arc<SurfaceFacts>` to current selection, per-root command bases and measurement provenance. Geometry and program recipes do not contain selected values. Bound paint views obtain them from a surface-scoped `SparseSecondaryMap`, populated during pane construction. Global node slot indices do not determine its capacity. Root titles capture the tabs identity during construction and read the current binding at execution; linking performs no item-to-pane search.

The command owner recognizes one successful topology-preserving Select. It shares structural membership, layout topology and the complete question inventory, updates the selected root's semantic fingerprint and reissues only the affected surface ticket. The contribution still answers the complete current Q. Reuse additionally requires equal actual effective bar policy, all measurement values/keys, selected outer minima, scroll/reveal, configuration, coordinates and relevant lifecycle/projection state. Nonprojecting Armed presses are eligible; geometry-producing gestures and unsupported cases use the sole complete compiler.

On a match, the exact facts, retained root layouts, hit/touch geometry, semantic receiver inventory and paint-program link survive. Installation mints fresh output bindings without layout solving, geometry construction/validation, hit emission/touch construction, semantic collection/sorting or program relinking. Old outputs keep their own selection, command basis and presentation ledger identity. Shared geometry grants no presentation/input authority.

The refactor removes proof-bearing target destinations, duplicate target-ID consistency checks, selected fields in pane/tab geometry and the temporary pane-selection join. `DropTargetLocation` derives target identity from structural intent. The existing resolver captures the live target, checks frozen-versus-live structure and evaluates actual current drop policy. There is no compatibility constructor, second complete compiler, generic dependency graph or fingerprint-history cache. Program construction inventories are empty after finishing/linking; most were already consumed by the previous builder, so this is not counted as new avoided construction.

The original workspace transaction copy remains. Updating the selected root fingerprint still copies/scans its node vector, sharing its immutable item/MRU/child/weight arrays. Updating the plan selection table still uses copy-on-write over the table. This is removal of unrelated derived work, not a constant-time workspace mutation claim.

The retained reveal check uses the original allocated local widths and the same prefix-range function as complete compilation. It does not reconstruct widths from translated rectangles, whose finite floating-point rounding can differ at nonzero origins.

## Breaking API migration

| Previous API | Current API |
| --- | --- |
| `DropDestination` and proof-bearing `DropTargetRecord::destination()` | `DropTargetLocation` and `DropTargetRecord::location()`; command authority belongs to the plan/resolver |
| Target constructor with independent ID and destination | `DropTargetRecord::new(location, availability, region, layer, visual)`; `location.id()` derives the identity |
| `PaneRecord::selected()` | `PresentationPlan::pane_selected(&pane)` |
| `TabRecord::selected()` | `PresentationPlan::tab_selected(&tab)` |

The runtime `PanePaintRecord::selected()` and `TabPaintRecord::selected()` remain convenient bound views for actual renderers. The plan inspection helpers resolve that plan's selection by node identity; supply geometry records obtained from the same plan. These helpers do not prove record membership or input authority. Repository callers migrate directly; no deprecated aliases preserve the removed representation. Methods that now read shared facts are ordinary methods instead of `const fn` where required. The public detached measurement and general frame/settlement contracts remain supported.

## Complete product timing

Milliseconds per sample. Idle/resize are one complete frame; click is the original press plus release. Each group/condition/case pools 48 raw samples with nearest-rank p50; every raw sample and p95/max summary is retained.

| Panes | Case | Group 1 before → after | Reduction | Group 2 before → after | Reduction |
| --- | --- | ---: | ---: | ---: | ---: |
| 16 | `idle` | 0.114958 → 0.111875 | 2.68% | 0.114459 → 0.111083 | 2.95% |
| 16 | `tab_click` | 0.355042 → 0.268041 | 24.50% | 0.355583 → 0.272833 | 23.27% |
| 16 | `surface_resize` | 0.202917 → 0.197667 | 2.59% | 0.199667 → 0.202208 | -1.27% |
| 128 | `idle` | 0.785709 → 0.740250 | 5.79% | 0.777750 → 0.741584 | 4.65% |
| 128 | `tab_click` | 2.589625 → 1.770083 | 31.65% | 2.569999 → 1.789000 | 30.39% |
| 128 | `surface_resize` | 1.590167 → 1.602666 | -0.79% | 1.594375 → 1.611208 | -1.06% |
| 1024 | `idle` | 7.356250 → 6.833500 | 7.11% | 7.152041 → 6.896334 | 3.58% |
| 1024 | `tab_click` | 25.222749 → 16.681041 | 33.87% | 25.072583 → 16.685166 | 33.45% |
| 1024 | `surface_resize` | 16.330875 → 16.236959 | 0.58% | 16.376375 → 16.231917 | 0.88% |

The two groups exceed the plan's 10% complete-click investment threshold. All compared product raw samples have identical frames, actual passes, content calls, visible contents, vertices, indices and draw-command counts. The original 16/128/1,024-pane layout, two tabs per pane, selected text/square content, styling, complete CPU meshes and presentation reporting are unchanged.

The small resize differences are mixed. A separate 128-pane resize ABBA follow-up, using the same frozen binaries and complete measured frame with 96 samples/12 warmups per process, gives pooled p50 **1.550583 → 1.523542 ms** (192 samples per condition, 1.74% lower). Per-process p50s are baseline 1.540542/1.553375 ms and candidate 1.524291/1.514000 ms. Its work fields also match. This follow-up does not replace the negative formal cells or establish a consistent resize speedup; the roughly 1% regression did not repeat in that check.

Tails remain mixed: group-1 1,024-pane resize maximum rises from 17.158 to 21.319 ms, and group-2 1,024-pane idle maximum rises from 8.899 to 13.041 ms. Group-2 16-pane resize p95/max rise from 0.203/0.215 to 0.222/0.275 ms. No universal tail improvement or latency guarantee follows.

The separate complete-product reference run uses the same final candidate and pinned Dear ImGui:

| 1,024-pane case | Candidate egui | Dear ImGui | Product ratio |
| --- | ---: | ---: | ---: |
| `idle` | 7.039208 ms | 1.139459 ms | 6.18× |
| `tab_click` | 17.132875 ms | 2.273668 ms | 7.54× |
| `surface_resize` | 16.714333 ms | 1.133458 ms | 14.75× |

These are full product/framework ratios, not pure docking-core ratios. Reference and frozen-binary comparison groups are separate runs. No renderer-free time is subtracted from these values to estimate a framework-only budget.

## Renderer-free complete update and settlement

The unchanged `frame_cost` workload measures the public semantic update, complete owning program recorder, confirmation/commit and separate settlement. It has no egui/font/tessellation/GPU/native-window work. Selection is one semantic update, distinct from the product's two physical click frames. Four per-process p50s are shown in execution order within each condition; these are **not pooled percentiles**.

| 1,024-pane update | Baseline process p50s, ms | Candidate process p50s, ms |
| --- | --- | --- |
| `idle` | 1.655, 1.608, 1.619, 1.619 | 1.264, 1.268, 1.268, 1.269 |
| `select` | 9.747, 9.561, 9.615, 9.706 | 1.793, 1.787, 1.804, 1.796 |
| `drag` | 12.429, 12.281, 12.548, 12.489 | 12.187, 12.228, 12.045, 12.223 |
| `measurement_change` | 10.995, 10.790, 10.985, 11.038 | 10.719, 10.664, 10.642, 10.755 |
| `outputs_in_flight` | 9.666, 9.653, 9.734, 9.802 | 8.349, 8.295, 8.415, 8.382 |

Idle settlement process p50s decrease from 0.512–0.520 to 0.187–0.192 ms; selection settlement decreases from 1.006–1.035 to 0.247–0.251 ms. Shared immutable equality short circuits also benefit these phases. All scales, cases, phases, p95s and maxima are archived. The workload emits process summaries rather than individual timing samples, so its percentiles cannot be recomputed from this archive. A small 1,024-pane measurement-change update maximum increase, 12.200 → 12.204 ms across captured processes, remains in the data.

The formal in-flight parameter is **one** older output: one untimed prefill and one timed update produce two pending outputs before settlement. That scenario also alternates bounds and tab intrinsic, unlike fixed-measurement selection. Its current candidate differs from the confirmed fallback, so the bounded retained-selection preparation is not established; the complete builder runs. Simply admitting an unconfirmed candidate would still fail measurement-value equality. The 8.30–8.41 ms result is not an isolated output-ledger cost or a proved lower bound. See the [source trace](2026-11-06-retained-facts/in-flight-followup.md).

## Allocation and retained storage

Active DHAT runs are separate from qualification timing, with six samples/two warmups and `--in-flight 1`. The following are summary medians for the update interval at 1,024 panes; values include the owning recorder and outputs.

| Case | Allocation events before → after | Requested bytes before → after | Live bytes at update end before → after |
| --- | ---: | ---: | ---: |
| `idle` | 2,793 → 2,696 | 17,747,440 → 17,646,080 | 22,742,712 → 22,252,736 |
| `select` | 68,729 → 15,492 | 46,398,268 → 19,121,728 | 30,661,376 → 22,899,080 |
| `drag` | 62,692 → 62,610 | 72,423,116 → 71,802,144 | 36,025,416 → 35,013,648 |
| `measurement_change` | 54,768 → 54,685 | 71,682,260 → 71,061,192 | 35,771,976 → 34,727,472 |
| `outputs_in_flight` | 68,748 → 53,330 | 46,401,340 → 43,813,840 | 52,022,360 → 50,005,448 |

Selection's captured cumulative prefix peak decreases from 32,019,424 to 23,701,960 tracked bytes; the in-flight case decreases from 53,382,664 to 50,572,984. These are case-prefix peaks including setup/warmup/earlier samples, not isolated update-window peaks or RSS. After settlement, live tracked selection storage is 14,615,880 → 13,888,248 bytes; in-flight storage is 14,616,256 → 13,888,624 bytes. Settlement allocation events/requests stay equal except drag, which adds three events/288 requested bytes.

The representation adds a surface-scoped sparse selection table, one bound command basis per root and eight bytes of original allocation width per strip member. It removes per-target command payloads and shares facts/receiver/layout owners across selected outputs. Reported totals cover current/fallback/in-flight ownership; they do not imply a fixed per-scene saving. General transaction copies, selected-root fingerprint-vector copies and current full measurements remain.

## Fresh CPU samples and remaining direction

Two separate eight-second macOS CPU captures use the frozen final candidate, after all timing/allocation/follow-up processes finish. They include initialization, warmup, untimed hover/geometry preparation, validation and disposal; they are not clean partitions of the timed frames. Each category merges nonoverlapping roots within that category, but categories overlap and **must not be added**. Inlining may hide work.

| Inclusive category | Idle, 5,851 main-thread samples | Click, 5,871 samples |
| --- | ---: | ---: |
| Scene compiler | 0 observed | 0 observed |
| Scene validation | 0.017% | 0.017% |
| Scene value comparison | 0.051% | 0.017% |
| Measurement/resources | 12.237% | 11.020% |
| Measurement-map insertion | 3.384% | 2.947% |
| Receiver queries | 8.443% | 7.324% |
| egui widget creation | 9.503% | 9.334% |
| egui child-UI creation | 4.170% | 3.713% |
| CPU tessellation | 11.793% | 10.594% |
| Call trees containing output audit | 17.006% | 14.648% |

The source/counter tests establish the avoided constructors; these samples support that mechanism but are not predicted percentage savings. Output-audit call trees include real pane/application rendering and cannot be treated as a removable auditing budget. The next useful targets are repeated receiver association and synchronous measurement-map assembly, with actual callback/content contracts retained. Numerical layout has no observed standalone samples in these captures; that does not establish zero cost or a theoretical floor.

For mixed changed-input/in-flight work, first separate actual geometry changes from fact-source eligibility. A bounded choice among already retained exact-compatible owners may be useful; changing output permissions or adding an unbounded history cache is unnecessary. No additional speedup is claimed for that proposal.

## Correctness, review and provenance

Final exact-source checks pass: **14 focused tests**, **2,186 workspace all-feature/all-target tests**, **292 default/335 serde host tests**, **556 native tests**, **10 doctests**, workspace/native Clippy correctness and suspicious checks, formatting, dependency boundary and diff checks. Focused tests are included in the workspace count. The existing opt-in layout probe and native wall-time test remain skipped.

The new regressions cover complete retained-plan versus conservative-builder equality; actual press/Arm/release and threshold activation; current selected/MRU/title with old-output immutability; consumed preparation restored after encoding restart, old-receipt rejection and dropped-frame rollback; equal bar/different drop rules with actual preview and old-output rejection; unequal selected minima; offscreen reveal; another item's changed minimum/intrinsic; valid fractional-origin reveal boundaries; and one-pane surface selection capacity under high global node indices. Full native, stale/unpresented, callback/final-hook mutation and lifecycle coverage also comes from the existing suites, not an exhaustive new combination test matrix.

Two read-only peer reviews inspect the actual implementation and fixes. They identified three bounded issues: absolute-rectangle range reconstruction, full-workspace selection capture per surface and a per-root pane search for title identity. All three source causes were removed and their regressions passed. The reviews confirm actual construction bypass and current binding/authority separation while explicitly identifying remaining nonconstant copying. A separate read-only evidence review independently recomputed all 1,728 formal product rows and 36 summaries, checked complete work equality, headless process/schema/selection direction, allocation claims, source/binary/result hashes, checks, serial timelines and archive correspondence; it found no qualification blockers.

All 565 recorded Rust/build/workload inputs match the validated snapshot `4d874443ffdae2220c70401fe6f46335d004823983887dd09b6e0bc13dcfbc20`; 41 differ from baseline. The baseline is the previous qualified executable with matching source content hashes, not a fresh clean-checkout rebuild. Its product/headless digests are `c00a38ed2552d942dd99048eb06c491d27b0881e41efc122c12d148e7ddfb518` and `444f05f82da82ca495604fd3ca1b2f050cd390d88741167e899e0235131ca77b`. Workload and pinned ImGui hashes remain unchanged. Final checks, review receipt and builds identify the same frozen source; the precommit Git label alone is not the provenance.

Product builds use the production profile with ThinLTO/one codegen unit. Headless uses release with explicit equivalent overrides. Cargo has one build job; nextest uses one test thread. Checks finish first. Each workload builds before its own serial timing groups; the headless build follows product timing. Active allocation profiling, the resize follow-up and CPU captures run afterwards, in that order. Product/headless groups use ABBA then BAAB; product has 24 samples/six warmups per process and headless has 48/six. No GPU/native-window/vsync, input-latency, RSS or isolated desktop/power-state result is claimed.

See the [evidence index](2026-11-06-retained-facts/README.md) for raw rows, summaries, source/binary receipts, commands, review reports and compressed logs/captures. Runtime binaries stay under `/tmp/dockspace-fearless-20261106-34ab23a/` and are not committed. All 49 protected user CI/review paths keep their starting hashes; `repo-ref/egui` is outside implementation ownership.
