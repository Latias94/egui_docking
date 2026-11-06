# Same-pass title resources and demand-driven focus lookup

Date: 2026-11-06. Baseline: `3803549e35bbed875728d491bf1da44a3c4149d3`. Status: qualified.

This follows [compiler-owned plan finalization](2026-11-06-compiled-finalization.md).

## Result and scope

The complete 1,024-pane product idle p50 decreases by 7.05% and 5.21% in two serial comparison groups. The corresponding two-frame click improves by 3.88% and 4.07%. This is an adapter resource-lifetime and materialization change. Every `crates/dockspace` source and the complete benchmark workload remain unchanged; no new renderer-free-core speedup is claimed.

The implementation makes three related changes:

- Each ordinary single-surface owner creates one call-local `PaintResources`, lets measurement populate it, then moves it into painting. The measured title and immutable `Arc<Galley>` become the actual painted resource for that pass. Painting still traverses the program and fills resources absent from measurement, including a possible drag ghost.
- The private per-item resources and per-bar row heights use `HashMap` because callers only use keyed entry/get operations. Neither map is iterated to decide visible order. Missing items retain their ordered `BTreeSet` and the surface program retains all paint order.
- The tab visual lookup is no longer built for every ordinary frame. A call-local `OnceCell<HashMap<...>>` materializes it on the first real keyboard-navigation or opt-in diagnostic query. It traverses the same program, filters the same Tab identities and preserves first-entry semantics. Repeated diagnostic queries share that one result, avoiding repeated full scans.

## Ownership and behavioral boundary

The resource set is created inside each actual pass, with the same `Ui`, style and pane owner used for immediate painting. It is never retained across passes, frames or outputs. Skipping measurement supplies a fresh empty set and painting obtains current resources. Measured missing items are collected before a no-paint branch, and program resource preparation still obtains actual painted items. The focus map is derived from the painted program rather than the full measurement roster.

`PaneView::title` now documents same-pass retention by the single-surface owners. A getter that changes on every call previously could measure one string and paint another; the new behavior intentionally paints the measured snapshot. The next pass reads it again. No arbitrary application-owned callback object is stored in these maps: resources contain an owned string, an immutable Galley owner and a missing flag.

Native public measurement and painting remain independent calls. Native measurement passes `None`, shapes and drops each title as before, and returns its original missing set. Native painting starts with an empty resource set. An initial draft retained an unused resource map for native; that was corrected before any formal timing. There is no cross-call native cache or new invalidation protocol.

Core requirements, layout projection, scene validation, receiver/input authority, affine execution receipts, final output checks and presentation settlement use their existing paths. This change does not reuse a stale scene as current input authority.

## Complete product medians

All values are milliseconds per measured sample. Idle and resize are one complete frame; `tab_click` sums the original press/release frames. Each table cell is a nearest-rank median of 48 raw samples in its group.

| Panes | Case | Group 1 before → after | Reduction | Group 2 before → after | Reduction |
| --- | --- | ---: | ---: | ---: | ---: |
| 16 | `idle` | 0.122041 → 0.108583 | 11.03% | 0.119250 → 0.118333 | 0.77% |
| 16 | `tab_click` | 0.358125 → 0.342583 | 4.34% | 0.361666 → 0.357625 | 1.12% |
| 16 | `surface_resize` | 0.196458 → 0.197584 | -0.57% | 0.196208 → 0.204083 | -4.01% |
| 128 | `idle` | 0.820042 → 0.783000 | 4.52% | 0.819875 → 0.781625 | 4.67% |
| 128 | `tab_click` | 2.676749 → 2.593666 | 3.10% | 2.677749 → 2.586667 | 3.40% |
| 128 | `surface_resize` | 1.617000 → 1.619292 | -0.14% | 1.657125 → 1.620875 | 2.19% |
| 1024 | `idle` | 8.007959 → 7.443625 | 7.05% | 7.960916 → 7.545875 | 5.21% |
| 1024 | `tab_click` | 26.636458 → 25.601667 | 3.88% | 26.872208 → 25.779042 | 4.07% |
| 1024 | `surface_resize` | 17.196625 → 16.615166 | 3.38% | 17.147458 → 16.670000 | 2.78% |

The small-layout result is mixed: 16-pane resize regresses by 0.57% and 4.01%, while its second-group idle change is only 0.77%. Tails are also mixed. At 1,024 panes group-1 idle maximum rises from 8.706 to 12.558 ms despite a better median/p95. At 128 panes group-2 idle p95/max rise from 0.951/1.236 to 1.430/2.414 ms. All raw samples and p95/max summaries are retained; these data do not establish a universal latency bound.

All sample counts, frames, actual passes, content calls, visible contents, vertices, indices and draw-command counts match exactly between before/after samples in both groups. The benchmark still uses all visible 16/128/1,024 panes, two tabs per pane, actual content text and square, ordinary product styling, real clicks and complete CPU tessellation.

The separately timed full-product reference in this qualification is:

| 1,024-pane case | Final egui product | Dear ImGui product | Ratio |
| --- | ---: | ---: | ---: |
| `idle` | 7.423917 ms | 1.113542 ms | 6.67× |
| `tab_click` | 25.533792 ms | 2.210376 ms | 11.55× |
| `surface_resize` | 16.556167 ms | 1.095916 ms | 15.11× |

These are complete framework/product numbers, not renderer-free docking-core ratios. They cannot be subtracted from the different headless workload to infer a framework-only cost. The remaining gap is material.

## Separate allocation experiment

The production CPU executable has no active allocation profiler. For allocation accounting only, external copies of the same complete product driver add DHAT and snapshots around the original frame interval. Both versions use identical instrumentation and byte-identical lockfiles; baseline library sources are checked against the baseline Git objects. The candidate library source delta is the same nine adapter paths. Timing values from active DHAT runs do not qualify CPU latency.

The experiment runs baseline/candidate/candidate/baseline, with six measured samples and two warmups per case at all three scales. Each 1,024-pane result below is the per-process median; both processes per condition agree. Click allocation counts sum both timed frame intervals and exclude the original preparation/hover frames and output disposal.

| 1,024-pane case | Allocation events before → after | Requested bytes before → after |
| --- | ---: | ---: |
| `idle` | 43,914 → 36,940 | 34,908,210 → 34,601,866 |
| `tab_click` | 162,415 → 148,467 | 97,509,460 → 96,896,772 |
| `surface_resize` | 95,311 → 88,337 | 78,349,950 → 78,043,606 |

Idle avoids 6,974 allocation events (15.88%) and 306,344 requested bytes per frame. The two-frame click avoids 13,948 events (8.59%) and 612,688 requested bytes. Live bytes at the measurement boundaries and the cumulative per-case prefix peak are unchanged in the captured samples at all scales. The request totals are not live heap or RSS. These counts include the complete product and draw data; they are not pure-core totals.

## Fresh attribution and remaining direction

Eight-second macOS CPU samples are separate from all formal timing, builds, tests and reviews. Sampling includes readiness, warmups, unmeasured click preparation/hover and output disposal, so its percentages are not exact fractions of the frame timer. Each category sums disjoint matching call-tree roots, but categories overlap each other; do not add columns. Inlined code may be charged to its caller.

| Sampled condition | Thread samples | Measurement/resources | Scene compiler | Scene validator | Layout solver | Paint-output audit | CPU tessellation |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| baseline `idle` | 6,369 | 15.09% | 0.00% | 0.03% | 0.00% | 15.47% | 10.68% |
| baseline `tab_click` | 6,387 | 25.16% | 9.28% | 4.31% | 0.80% | 11.60% | 7.56% |
| candidate `idle` | 5,806 | 11.35% | 0.00% | 0.05% | 0.00% | 14.92% | 10.99% |
| candidate `tab_click` | 5,841 | 23.03% | 9.84% | 4.45% | 0.82% | 11.83% | 7.60% |

No standalone compiler or layout-solver frame appears on idle, consistent with existing complete-plan reuse. The sampled title-resource category shrinks, while receiver joins, egui widget registration/response work, Galley/shape comparison and tessellation remain. On clicks, scene compilation and validation are visible, but the numerical solver has a much smaller standalone share. This does not prove that every validation is necessary or that sample absence means zero work.

The next architecture questions are consequently narrower:

1. Can a local selection rebuild only affected semantic/record views while sharing unchanged layout and chrome? Selection can change selected minima and tab policy, so a new plan must compare actual dependencies. Old target source fingerprints and output/input capabilities cannot simply be retained.
2. Can paint-time receiver identity joins and repeated immutable-plan comparisons consume already linked records instead of comparing large semantic IDs on every frame? The final idle sample still shows `PresentationHitRegionKind::cmp` and `SurfacePaintPlan::receiver` prominently. Any change must retain the exact current hit inventory and foreign-wrapper behavior.
3. Can the framework adapter reduce repeated widget/response/output work while preserving real application callbacks and end-of-output verification? Audit still appears in roughly 15% of sampled idle CPU, and tessellation in roughly 11%; neither explains the whole product gap.

This round does not attempt speculative geometry reuse or weaken any checks to close the full ImGui ratio. The existing renderer-free baseline remains the previous qualification; all of its core sources are byte-identical here.

## Iteration and correctness evidence

The first formally measured candidate reused titles but kept the original ordered maps and eager focus map. Its 1,024-pane idle p50 improved by 4.85% and 2.57%, below this round's two-group 5% target. Its complete raw rows, tails, build/source metadata and exact patch are preserved under `title-reuse-only/`. A separate idle-only pilot compared that candidate with private hash lookups before the final demand-driven focus change. The pilot is exploratory, not the final qualification.

Final exact-source checks pass:

- 2,172 workspace all-feature/all-target nextest tests; the existing opt-in layout probe is skipped.
- 135 adapter all-feature/all-target tests, including same-pass changing-title/next-pass refresh for both single-surface owners and a real pointer/ArrowLeft focus-plus-queued-selection regression.
- 292 default and 335 serde host tests; 556 native all-feature/all-target tests, with the existing wall-time case skipped.
- 10 doctests, correctness/suspicious Clippy for workspace and native, `cargo fmt --all -- --check`, dependency boundary and diff checks.
- Independent read-only code reviews and a final review of hash lookup and demand-driven map construction found no blockers. Reviews did not run during formal timing.

A fork product harness nextest attempt compiled successfully but found zero tests and exited 4. That attempt is retained separately and is not counted as a passing test gate. The production product executable and its runtime workload checks supply the actual fork product coverage.

The new title regression uses one pass per run, and the keyboard regression uses the owned owner with two tabs. Existing discard/native-diagnostic/ghost/popup tests pass, but the new regressions do not exhaust every combination. The title getter callback frequency intentionally changes as documented. No native window/GPU/input-latency or RSS qualification is claimed.

## Reproduction and provenance

Run the standalone evidence check:

```text
python3 docs/performance/2026-11-06-title-resources/verify_evidence.py
```

It recomputes final and first-candidate raw product percentiles, complete workload equality, separate allocation summaries, exact adapter-only source scope, and non-overlap of formal CPU runs with recorded builds/tests/reviews/profiling/active DHAT. `SHA256SUMS` covers the archived evidence. The check reads archived data and does not run a benchmark or parse Rust.

The [evidence directory](2026-11-06-title-resources/README.md) retains source/executable hashes, raw rows, profiles, reviews, check receipts, identical allocation-driver instrumentation and local orchestration helpers. Large build/profile logs are gzip-compressed. Runtime binaries and full baseline source extraction remain at `/tmp/dockspace-refresh-20261106-3803549/` and are not committed. The baseline product is the previous qualification's frozen candidate, checked against all 565 benchmark/source paths and the `3803549` Git objects before use.

Production builds use ThinLTO and one codegen unit with Rust 1.95.0; all Cargo work is serial and reuses the existing target. Formal product groups run ABBA then BAAB, 24 samples and six warmups per process at all three scales. The public product runner separately supplies four egui/ImGui reference runs. Power/desktop scheduling is not isolated. Source hashes, rather than the precommit Git label embedded in build metadata, identify the exact qualified tree. The 49 protected user CI/review paths match their starting hashes.
