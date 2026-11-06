# Link drop-target receivers when installing a scene

Date: 2026-11-06. Baseline: `293557126154d3ce11ec0a7294e119cbe6e6abe3`. Status: qualified.

This follows [same-pass title resources](2026-11-06-title-resources.md).

## Result

The complete 1,024-pane product idle p50 decreases by 3.21% and 3.18% in two serial groups. The renderer-free complete update also improves: four baseline idle process medians span 1.859–1.903 ms, while four candidate medians span 1.633–1.655 ms. The same complete program, content and output obligations remain in both workloads. This removes repeated drop-target identity resolution during program execution; other public receiver queries still use their original resolution paths.

The change deliberately trades retained storage for repeated CPU work. Each retained 1,024-pane scene adds 73,760 tracked bytes in the headless workload. Changed-scene updates add 13 allocation events and 270,272 cumulative requested bytes. Idle and settlement allocation counts/requests stay unchanged. These costs are part of the result, not an unchanged-memory claim.

## The duplicated work

Before this change, scene installation already visited every immutable surface-program step to prove executability. For each nonvisual DropTarget instruction it found the exact canonical hit region, rejected passive/missing regions and checked that its center was representable. Every program execution then repeated the same semantic-ID binary search and center calculation. Cached idle reused the full scene but still paid those joins on every frame.

`link_surface_paint_program` retains the same installation checks and records each target’s canonical row and checked center, in program order. Its private `SurfacePaintProgramLink` is held by `SurfacePlanScene` beside the exact immutable plan and hit manifest. Installation creates it after canonical ordering, touch compaction and any hit-geometry reuse. Each new scene links against its own final manifest; a shared or historical geometry allocation alone does not supply link eligibility.

`prepared_paint_plan` borrows the plan, manifest and link from that one candidate. Runtime traversal consumes one linked entry only when it encounters a DropTarget instruction, reads the region with checked row access and uses the retained center. It still constructs each descriptor with the current output ticket and the current region’s identity, lanes and bounds. Missing/leftover entries fail traversal; a sink error retains its existing precedence and no execution receipt is issued on failure.

The private table has one row/center pair per relevant target instruction. It is rebuilt only when installing a new scene and shared by scene clones/retained outputs. It introduces no cache key, invalidation history, alternate input authority or public constructor. Ordinary `receiver_for_*`, foreign record wrappers, pointer/touch resolution, semantic requirements and reducers retain their existing paths.

For arbitrary/malformed test plans, the original per-step lookup/check path remains available by omitting the link. The regression compares the complete linked target-descriptor sequence to this original path across repeated and changed bounds. Existing counters assert zero hit-identity comparisons in linked program traversal and positive comparisons in the original path. Missing-receiver/malformed-program tests still reject before invoking the renderer.

## Complete product medians

All values below are milliseconds per measured sample. Idle/resize are one complete frame; clicks sum the original press and release frames. Each group pools 48 retained raw samples per condition/case using nearest-rank p50.

| Panes | Case | Group 1 before → after | Reduction | Group 2 before → after | Reduction |
| --- | --- | ---: | ---: | ---: | ---: |
| 16 | `idle` | 0.120250 → 0.113958 | 5.23% | 0.114792 → 0.112917 | 1.63% |
| 16 | `tab_click` | 0.373459 → 0.346125 | 7.32% | 0.357374 → 0.352000 | 1.50% |
| 16 | `surface_resize` | 0.207000 → 0.202959 | 1.95% | 0.202875 → 0.200708 | 1.07% |
| 128 | `idle` | 0.808083 → 0.762583 | 5.63% | 0.787083 → 0.769584 | 2.22% |
| 128 | `tab_click` | 2.703042 → 2.595084 | 3.99% | 2.633624 → 2.576875 | 2.15% |
| 128 | `surface_resize` | 1.627458 → 1.586750 | 2.50% | 1.713291 → 1.590416 | 7.17% |
| 1024 | `idle` | 7.508750 → 7.267500 | 3.21% | 7.567667 → 7.327292 | 3.18% |
| 1024 | `tab_click` | 25.825250 → 25.347084 | 1.85% | 25.992667 → 25.461416 | 2.04% |
| 1024 | `surface_resize` | 17.085916 → 16.476000 | 3.57% | 16.732125 → 16.681916 | 0.30% |

All compared raw samples have identical frames, actual passes, content calls, visible contents, vertices, indices and draw-command counts. The complete 16/128/1,024-pane workload, two tabs per pane, content text/square, ordinary product styling, real clicks and full CPU draw data remain unchanged.

Medians improve in all captured cells, but small gains and tails are mixed. Group-2 1,024-pane resize improves only 0.30%. At 16 panes, group-1 resize p95/max rise from 0.280/0.513 to 0.482/0.661 ms, and group-2 resize p95/max rise from 0.211/0.299 to 0.311/0.436 ms. Group-2 128-pane idle maximum rises from 1.025 to 1.129 ms. Every raw sample and p95/max summary is retained; no universal tail guarantee follows.

The separately timed complete-product reference at 1,024 panes is:

| Case | Candidate egui | Dear ImGui | Candidate / ImGui |
| --- | ---: | ---: | ---: |
| `idle` | 7.164458 ms | 1.105292 ms | 6.48× |
| `tab_click` | 25.397584 ms | 2.197958 ms | 11.56× |
| `surface_resize` | 16.857292 ms | 1.121500 ms | 15.03× |

These are full framework/product ratios, not pure docking-core ratios. The remaining gap is large. The renderer-free workload differs from this product workload, so their timings cannot be subtracted to estimate a framework-only cost.

## Renderer-free complete update

The existing public `frame_cost` workload executes and records the complete ordered program, commits actual core outputs and settles their receipts. It uses no egui, epaint, font, GPU or native-window work. Selection is a public semantic command, distinct from the two-frame product click; measurement change modifies bounds and intrinsic tab width. The in-flight case retains one older output.

These are separate per-process p50 values, in execution order within each condition, in milliseconds. They are not pooled percentiles:

| 1,024-pane update case | Baseline process p50s | Candidate process p50s |
| --- | --- | --- |
| `idle` | 1.859, 1.883, 1.880, 1.903 | 1.634, 1.633, 1.635, 1.655 |
| `select` | 9.999, 9.804, 9.950, 9.970 | 9.689, 9.645, 9.697, 9.875 |
| `drag` | 12.619, 12.516, 12.657, 12.749 | 12.342, 12.373, 12.500, 12.848 |
| `measurement_change` | 11.135, 11.068, 11.265, 11.186 | 11.006, 10.910, 10.956, 11.049 |
| `outputs_in_flight` | 9.951, 9.945, 10.130, 10.397 | 9.677, 9.686, 9.688, 9.996 |

Selection improves modestly and its process ranges overlap. Drag is mixed: the last candidate median is 12.848 ms versus the baseline range 12.516–12.749 ms. Candidate measurement-change maximum reaches 19.390 ms, while the largest baseline maximum is 12.728 ms; candidate drag maximum reaches 16.579 ms versus 15.301 ms. Idle settlement stays approximately 0.51–0.53 ms. All process p50/p95/max values, all scales and both update/settlement phases remain archived. The workload emits summaries rather than individual timing samples, so its percentiles cannot be independently recomputed from this archive.

## Allocation and retained-storage cost

Active DHAT accounting is separate from CPU runs. The workload uses six measured samples, two warmups and one older in-flight output, at all three scales. CPU timing uses inactive DHAT; active profiling does not qualify latency. Values below are measured summary medians for the update interval.

| 1,024-pane case | Allocation events before → after | Requested bytes before → after | Live bytes at update end, before → after |
| --- | ---: | ---: | ---: |
| `idle` | 2,793 → 2,793 | 17,747,440 → 17,747,440 | 22,668,952 → 22,742,712 |
| `select` | 68,716 → 68,729 | 46,127,996 → 46,398,268 | 30,513,856 → 30,661,376 |
| `drag` | 62,679 → 62,692 | 72,152,844 → 72,423,116 | 35,877,896 → 36,025,416 |
| `measurement_change` | 54,755 → 54,768 | 71,411,988 → 71,682,260 | 35,624,456 → 35,771,976 |
| `outputs_in_flight` | 68,735 → 68,748 | 46,131,068 → 46,401,340 | 51,801,080 → 52,022,360 |

In this workload the stored target pairs and Arc add 73,760 tracked bytes per scene. Ordinary idle has one retained scene; changed-scene update end retains two, giving a 147,520-byte delta. The in-flight update end retains three, giving a 221,280-byte delta. Captured prefix peaks rise by the same corresponding amounts. Settlement returns to the one-scene delta and its allocation counts/requests are unchanged.

The 13 extra allocations/270,272 requested bytes on rebuilding updates include Vec growth/shrink and the shared link owner. This is less than 0.03% of the roughly 55,000–69,000 update allocations in the shown rebuild cases, but it remains an increase. The design keeps one straightforward private vector rather than adding a second counting/provenance structure solely to remove that small installation cost. Requested bytes are cumulative requests; boundary live values and cumulative prefix peaks are not RSS. The recording sink and outputs are included in these totals.

## Validation and provenance

Final exact-source validation passes: 59 focused paint/lifecycle/hit tests; 2,173 workspace all-feature/all-target tests; 292 default and 335 serde host tests; 556 native all-feature/all-target tests; 10 doctests; correctness/suspicious Clippy for workspace and native; formatting, dependency boundary and diff checks. The existing opt-in layout probe and existing native wall-time test remain skipped. The focused tests are included in the workspace count.

An independent read-only implementation review found no blockers. A separate read-only evidence review independently verified the arithmetic, per-process/merged rows, source and binary provenance, checks and timing separation without blockers. It checked preflight acceptance, final manifest row ordering/compaction, co-owned immutable lifetime, descriptor/current-output binding, sink error precedence, cloning and retained scenes. The new regression directly covers repeated/changed bounds; passive/extreme-center/compaction/rollback combinations were assessed statically and through the existing broad suite, not exhaustively enumerated by the new test.

The baseline product is the previous round’s frozen candidate, verified against all 565 relevant source/build/workload paths and baseline Git objects. The baseline headless executable comes from the earlier compiled-finalization qualification: all 372 relevant core/harness/build inputs are byte-identical at `2935571`, and its executable digest matches the original environment. The intervening commit changed only the egui adapter. Full provenance is retained rather than rebuilding a baseline from the modified tree.

Product builds use the production profile, ThinLTO and one codegen unit. Headless builds use release with explicit ThinLTO/one-codegen-unit overrides. Cargo and test execution are serial, reuse existing targets, and finish before formal timing. Product and headless groups run ABBA then BAAB; product has 24 samples/six warmups per process and headless has 48/six. Product reference uses the unchanged serial public comparison runner. Neither desktop scheduling nor power state is isolated. No GPU, native presentation, input latency or RSS result is claimed.

Run the standalone archive check:

```text
python3 docs/performance/2026-11-06-linked-targets/verify_evidence.py
```

It recomputes product percentiles and complete work equality, checks headless case/output coverage and published summaries, validates exact source scope/check snapshots and recorded timing separation. The check reads data only; it does not rerun Cargo or infer Rust semantics. See the [evidence index](2026-11-06-linked-targets/README.md). Runtime binaries remain at `/tmp/dockspace-linked-receivers-20261106-2935571/` and are not committed. The candidate build label is precommit; exact source hashes identify the qualified tree. All 49 protected user CI/review paths retain their starting hashes.

## Remaining direction

This links only nonvisual DropTarget program steps. Pane, tab, tab-bar, splitter and junction receiver queries still resolve semantic identities during adapter painting. Extending row binding to those records is a plausible next step, but wrappers can be passed to another current plan, so any fast path must preserve existing foreign-record behavior. A link must bind actual current-plan membership rather than trusting a wrapper’s source.

Local selection still rebuilds the affected surface’s scene. Selected pane minima and tab policy can legitimately change geometry, and target source fingerprints include selection/MRU. Sharing unchanged layout/chrome therefore needs dependency-based reuse while producing fresh current-output/input authority. This round adds no speculative stale-scene reuse and does not establish that the remaining cost is a theoretical lower bound.
