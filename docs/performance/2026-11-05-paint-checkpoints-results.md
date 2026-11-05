# Incremental paint checkpoints and immutable output ownership

Dates: 2026-11-05–06. Base: `36f3fb0551a00e6cb235b58f804442d8beaebf3a`.
Core: `4d811d0`; adapter, fork pin and product tooling: `e73d86a`;
spatial follow-up: `31b1759`.
Status: qualified; final production performance, all 22 follow-up regression gates
and independent review passed.

## Final matched production comparison

The final implementation meets the 1,024-pane p50 limits of 12 ms idle, 35 ms
per two-frame click, and 20 ms resize in **both** final ABBA groups. Each group
uses 24 samples and six warmups per process, at 16/128/1,024 panes, pooling
48 samples per product/case/size. The pooled results below contain 96 samples
per product/case/size. Units are milliseconds per sample.

| 1,024 panes | Previous fork p50 | Final p50 | Reduction | Final p95 | Final maximum | Dear ImGui p50 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Idle | 31.073 | 8.302 | 73.3% | 8.947 | 9.595 | 1.088 |
| Click, two frames | 82.009 | 30.241 | 63.1% | 34.177 | 35.713 | 2.195 |
| Resize | 42.223 | 18.915 | 55.2% | 20.047 | 25.116 | 1.089 |

| Final group | Idle p50 | Click p50 | Resize p50 | Complete raw rows |
| --- | ---: | ---: | ---: | ---: |
| qualified-twenty-one | 8.315 | 30.346 | 18.923 | 864 |
| qualified-twenty-two | 8.296 | 30.136 | 18.865 | 864 |

The p50 target is not a tail-latency guarantee. Final pooled resize p95 is
20.047 ms and maximum 25.116 ms; all samples remain in the raw data. No claim
is made that every tail sample or small-scale percentile improved. Earlier
groups with larger tails are also retained as intermediate observations.

The matched previous-fork control and final Rust use release optimization,
ThinLTO and one codegen unit. Dear ImGui uses `-O3 -DNDEBUG -flto=thin` on the
same Apple Clang 21 toolchain. The machine reports ARM64 macOS 27.0; Rust is
1.95.0 (`59807616e`, LLVM 22.1.2). Builds and CPU sampling did not overlap formal
measurement. Cargo and nextest ran serially with one job.

These numbers describe **the fork backend with the production profile**.
Official egui retains the exact snapshot fallback; it cannot supply append
mutation evidence. Library users configure optimization in their application
workspace: dependency profiles are not inherited. The runner defaults to the
official backend and ordinary release, and requires explicit
`--backend fork --profile production` for this condition.

## Final official-egui control

The same final source (`31b1759`) was also built against official egui with the
production profile. One separate complete ABBA group retained 864 raw rows and
48 samples per product/case/size, with 24 samples and six warmups per process.
Its 1,024-pane results are below; all units are milliseconds per sample.

| Product | Scenario | p50 | p95 | Maximum |
| --- | --- | ---: | ---: | ---: |
| Official-egui dockspace | Idle | 28.086 | 28.790 | 28.891 |
| Official-egui dockspace | Click, two frames | 70.552 | 74.502 | 78.394 |
| Official-egui dockspace | Resize | 39.146 | 40.504 | 40.724 |
| Dear ImGui | Idle | 1.143 | 1.260 | 1.496 |
| Dear ImGui | Click, two frames | 2.272 | 2.619 | 2.853 |
| Dear ImGui | Resize | 1.144 | 1.450 | 1.591 |

The official path preserves exact snapshot validation and exceeds the selected
p50 limits. The qualifying result requires the pinned fork's mutation evidence
as well as the core changes and production settings. Its complete workload
counters match both old-fork controls and both final fork groups per sample;
27,648 comparisons found no differences. Nearest-rank statistics, all process
output, final source hashes and both executable hashes were verified separately.
All scales and build/run intervals are archived in `official-current-final/`.
The earlier `official-final/` observation remains in the archive as history.

## Work and comparison boundaries

The original Rust product workload and Dear ImGui driver are byte-for-byte
unchanged from the base commit. The fork harness points directly to the original
Rust example. Both products build their complete balanced tree, two tabs per
pane, every selected content body and full CPU draw data. A click is a real
press/release sequence and sums two complete frames. Resize alternates width
16,384/16,448, keeping all panes visible. No geometry, integrity check, callback
or tessellation was removed to meet a time threshold.

All 1,728 final raw rows match the previous-fork control **per sample** for
frames, passes, content calls, visible contents, vertices, indices and draw
commands, including the original click command-count variation. Idle/resize
have one pass and click has two. The unchanged workload itself checks input
effects and complete content. This numerical evidence complements the regression
suite; counts alone cannot establish geometric or input equivalence.

Dear ImGui remains substantially faster: final pooled p50 ratios are about
7.6x idle, 13.8x click and 17.4x resize. These are complete product CPU costs,
including each product's own layout, fonts and mesh generation, rather than an
isolated docking-algorithm comparison. They exclude GPU upload, native window
presentation, vsync and physical input latency. No heap/RSS ratio is qualified.

## All final scales

Pooled final ABBA groups; 96 samples per row, milliseconds per sample.

| Product | Panes | Scenario | p50 | p95 | Maximum |
| --- | ---: | --- | ---: | ---: | ---: |
| egui_dockspace | 16 | idle | 0.114 | 0.134 | 0.345 |
| egui_dockspace | 16 | tab_click | 0.387 | 0.594 | 0.751 |
| egui_dockspace | 16 | surface_resize | 0.205 | 0.264 | 0.392 |
| egui_dockspace | 128 | idle | 0.836 | 1.276 | 1.757 |
| egui_dockspace | 128 | tab_click | 3.016 | 3.475 | 3.723 |
| egui_dockspace | 128 | surface_resize | 1.822 | 2.392 | 3.184 |
| egui_dockspace | 1024 | idle | 8.302 | 8.947 | 9.595 |
| egui_dockspace | 1024 | tab_click | 30.241 | 34.177 | 35.713 |
| egui_dockspace | 1024 | surface_resize | 18.915 | 20.047 | 25.116 |
| dear_imgui | 16 | idle | 0.017 | 0.030 | 0.087 |
| dear_imgui | 16 | tab_click | 0.033 | 0.034 | 0.046 |
| dear_imgui | 16 | surface_resize | 0.017 | 0.020 | 0.023 |
| dear_imgui | 128 | idle | 0.127 | 0.140 | 0.143 |
| dear_imgui | 128 | tab_click | 0.260 | 0.323 | 0.456 |
| dear_imgui | 128 | surface_resize | 0.135 | 0.145 | 0.288 |
| dear_imgui | 1024 | idle | 1.088 | 1.301 | 1.608 |
| dear_imgui | 1024 | tab_click | 2.195 | 2.452 | 3.102 |
| dear_imgui | 1024 | surface_resize | 1.089 | 1.306 | 1.708 |

## What changed and why

The original callback audit copied and compared growing graphics prefixes. The
owned egui fork now freezes lazy checkpoints before opaque mutable graphics
access escapes or an internal Painter write touches an earlier index. Ordinary
appends inspect only new descriptors. Tests count actual comparisons at 512 and
1,024 callbacks, demonstrating linear work on that path. Exact fallback remains
for opaque edits, with self-equality progress and strong Galley ownership scoped
to the synchronous paint. NaN, debug panic, length/field order, viewport rebuild,
foreign layers and opaque Drop reentry remain checked.

The adapter selects the fixed fork auditor through inherent method resolution
and a private official fallback trait. There is no application-installable
provider or registry. Every callback runs once without a Context lock, cumulative
failure is recorded before payload release, and normal child-Ui destruction
stays within the audited interval. Unwinding retains snapshot-before-child
release. The fork is published at `60c40304dab75305d2ac0f54a1cabfc354669006` on
`perf/paint-append-checkpoints-0.37`.

Core changes remove copying and repeated derived work: borrow large plan query
receivers and hit keys; seal static paint-program executability with its exact
immutable plan/manifest; retain one tab resource owner per Item; use one sorted
target catalog for duplicate checks, joins and final Arc-preserving ordering;
and mark magnetic-target coverage during those same exact joins. Validation
still follows original input order and chooses the same duplicate/error class.

Drag-source preparation reuses a workspace index only when both exact weak
allocation provenance and workspace version match. Otherwise it performs the
original live validation. Node generation, root membership, complete fingerprint
equality, partial-detachment policy and error order remain checked. Same-version
publication windows and Arc::make_mut dissociation have dedicated regressions.

Hit geometry is shared only after complete emission replay, full field/raw-bit
equality, count/tape success and exact consumer-coupled touch inputs. Compaction
that removes a row disables replay hints. New output and semantic tickets remain
independent. When geometry changes but every emitted identity still matches,
only the old canonical permutation is reused; all current geometry and touch
fragments are rebuilt. Touch candidates use a spatial median bounds hierarchy,
then restore original neighbor order and the existing dense fallback. The final
follow-up reuses only the private spatial leaf permutation at the same row count;
every current leaf and parent bound, exact query and fragment is rebuilt before
publication. An arbitrary bijective leaf ordering is correct even when geometry
or allocation/protected membership changes; different counts rebuild the ordering. Its
worst-case query complexity remains quadratic for adversarial overlapping data;
the improvement does not establish a universal logarithmic-query bound.

Resource preparation now visits only tab/popup/ghost requests from the same
sealed program, preserving callback order, duplicates and identities without
building unrelated receiver descriptors. Full paint execution still resolves
and checks the complete program. Private drag-source identities and borrowed
drop-target paint records reduce the measured surface-program step from 264 to
184 bytes on this build. The owned descriptor remains available unchanged.

Finally, an owned run encapsulates FullOutput immediately after all output
hooks return. Its private wrapper exposes no mutable shapes or FullOutput.
A complete preflight validates every original marker, descriptor, pixel scale
and paint-order predicate. Settlement may consume that evidence only for the
same unique retained capture identity; other proofs use the complete original
check. Global participant/order, poison, emission and execution-receipt checks
still run. Only texture deltas can be taken/restored while encapsulated. Local
declaration order retains texture retirement and ordered opaque-payload release.
This removes duplicate validation of an output which ownership keeps immutable.
The generic caller-owned `run_ui` path retains its original validation boundary.

## Public API migration

Ordinary calls to borrowed SurfacePaintPlan query methods keep their syntax.
Function-item adapters taking owned plans need closures, for example
`plans.map(|plan| plan.surface())` instead of `plans.map(SurfacePaintPlan::surface)`.

`SurfacePaintProgramStep::DropTarget` now carries `DropTargetPaintRecord<'_>`.
Hosts which store or register an owned receiver call `target.receiver()`.
Ignoring the payload keeps the same match arm. Complete identity, role, passive
and finite-center checks happen before the visitor receives the borrowed record.
Repository host consumers are migrated. `visit_tab_resources` is an additional
read-only API and cannot mint execution receipts.

## Experiments and attribution

The following ordinary-release experiments precede build-profile changes.
Each contains one complete 864-row ABBA comparison; units are milliseconds.

| Condition | Idle p50 | Click p50 | Resize p50 |
| --- | ---: | ---: | ---: |
| Previous fork | 30.966 | 83.320 | 43.530 |
| Incremental callback audit | 11.921 | 45.438 | 24.315 |
| Borrowed hit keys, sealed preflight, shared resources | 10.150 | 43.306 | 23.090 |
| Borrowed large-plan queries | 9.689 | 41.533 | 22.120 |
| Shared target sort and source-index reuse | 9.553 | 37.905 | 22.457 |
| Contiguous target keys | 9.549 | 36.851 | 21.361 |
| Exact geometry sharing | 9.571 | 34.218 | 21.889 |

An interleaved four/five/five/four confirmation retains 576 raw rows and supports
the contiguous target-key selection. Its stale summary label was repaired by
recomputing the unchanged complete raw CSV, without rerunning measurements.

Subsequent frozen experiments are retained as intermediate observations, not
independent attribution for every code edit. Large contiguous hit-ID keys and
an additional target HashMap did not improve resize and were removed. Full LTO
also failed to improve the result and was removed from the delivered profile.
`final-production-one` is that rejected full-LTO experiment, despite its early
name. `qualified-one` is an earlier candidate which missed the resize target;
it is **not** counted as a final qualifying group. Candidate-sixteen's separate
Galley proof memo was also replaced by exclusive FullOutput ownership. The initial
`qualified-two`/`qualified-three` groups passed, but a post-qualification build
changed executable identity and `qualified-final-one`/`qualified-final-two`
missed resize at 20.230/20.346 ms. Those results triggered further work rather
than completion. Center-based spatial partitioning and topology-only reuse
then produced the actual final groups `qualified-twenty-one` and
`qualified-twenty-two`. Both use the exact final frozen production executable
`47db8ca79dab9b4ae0b53fe19fd69cc40dc94d01efc710ee5bfdd3d7ffc5a12a`.
The extra hit HashMap trial in candidate-nineteen was also removed.

The final source gain is compared with the previous fork under matching
ThinLTO settings. Ordinary-release and profile gains are reported separately;
no ThinLTO gain is attributed solely to source changes.

## Qualification and evidence

Final code, resource traversal, geometry/order reuse, smaller program payloads,
spatial partitioning and output ownership have independent read-only review
receipts. All 22 required follow-up command groups passed for `31b1759`,
including the ordinary fork adapter and release invariants. Results include:

| Gate | Passed tests |
| --- | ---: |
| Workspace, all features and targets | 2,145 |
| Host consumer, default / serde | 292 / 335 |
| Official-egui consumer | 24 |
| Product consumer, default / serde | 114 / 119 |
| Native, all features and targets | 556 |
| Ordinary adapter with fork, all features and targets | 133 |
| Release critical invariants | 121 |
| Workspace doctests | 10 |
| Extra ignored native release wall-time service case, at e73d86a | 1 |

Three correctness/suspicious Clippy groups, strict workspace/native docs,
four formatting groups, dependency boundaries and diff checks also passed.
The native debug group retains one existing ignored wall-time case. Its extra
release execution passed on `e73d86a`, before the spatial-only follow-up.
Physical X11/display tests are not
qualified on this macOS environment.

The first workspace run found one stale exact work-count assertion: index reuse
now performs two fingerprint builds instead of four. Its expectation was updated
to exactly two, and the full workspace run passed. Two migrated host match arms
were formatted. A snapshot-only dependency resolution initially hit a locked
source change and then a yanked-version issue under generate-lockfile; full
metadata resolution reused existing pins offline. Only the expected egui/winit
packages changed from registry to the pinned forks. Main Cargo.lock is unchanged.
All failed attempts and successful final commands are retained.

The evidence directory is
[`2026-11-05-paint-checkpoints/`](2026-11-05-paint-checkpoints/).
It retains raw/summary/process CSV, environment and build commands, source
inventories, compressed CPU call trees, review receipts and qualification logs.
Portable evidence contains hashes rather than large executables. Runtime frozen
binaries remain under `/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/`.
`SHA256SUMS` covers every other file in the evidence directory. The final review's
portable-path finding is resolved by byte-exact source-evidence copies, and its
pending-gate note is closed by the completed 22-group follow-up records; see
`provenance/final-review-closure.json`.
The intermediate candidate-six executable was not preserved before its shared
target was overwritten; its measurements, build log and source inventory remain.
No claim is made that every historical executable is still available.

The original baseline build command lacks standalone UTC bounds. Serial tool
completion establishes build-before-freeze-before-run, but an exact interval
cannot be reconstructed. Final builds and timing commands have explicit UTC
bounds. Some intermediate source_commit fields name base `36f3fb0`; candidate
content is identified by the accompanying source hashes. A later test-only
qualification update corrects an old four-build work-count assertion to exactly
two, matching the verified index reuse; production code is unchanged.

The prototype fork's 570 input hashes were independently matched against the
published fork: 458 Git blobs and 112 Git LFS content hashes. Path and Git package
metadata remain distinct in those conditions. Compact artifact comparisons
contain 60 artifact-key records and 59 unique package IDs; these are not 60
unique packages.

CPU sampling was separate from formal timing and includes readiness, warmup,
untimed diagnostics and disposal. Nested sampled categories cannot be added as
precise frame-time percentages. The previous hot path had large cumulative
cost in scene preparation and validation, beyond ordinary layout projection;
future work can continue attribution there using this unchanged product driver.
