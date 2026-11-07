# Renderer-free docking refactor: final measurements

The retained runtime is `0e441c8`. Its complete-operation p50 meets the pinned
ImGui twofold threshold in **30 of 42 required group/cells**. All 128- and
1024-pane conditions pass; twelve 16-pane conditions remain above the target.
Numerical completion is not claimed. Two independent reviews support ending
this bounded search round after final remote qualification, with the remaining
misses preserved. They do not establish optimality or impossibility.

## Fixed comparison

The workload uses 16/128/1024 total panes, one/four logical views, resize,
center move, perpendicular edge split and supported cross-view move. Both
products perform real request capture, docking mutation, all-root layout and a
fresh complete owned common export within the timer. Rust also gathers fresh
leaf constraints. There is no rendering, egui frame, native host or GPU work.
The C++ harness invokes actual docking code from ImGui
`d88f238d1d363ef387382b2e29ad12cf96f826ac`, including root update and layout.

Each comparison has two serial ABBA/BAAB groups, 48 samples and eight warmups
per process/cell, giving 96 observations per condition/group/cell. Full state
vectors and digests agree. The primary threshold is complete-operation p50;
p95 and maximum observations remain in the evidence and are not acceptance
substitutes. No separate held-out workload was configured; selection and
reporting use this declared matrix. These are observed sample statistics on
one machine; confidence intervals and cross-machine performance confirmation
were not calculated. The host is Apple M4/16 GiB with Rust 1.95.0 on
aarch64 macOS; the [metadata receipt](2026-11-07-twofold-result/host-metadata.json)
is captured after the campaigns. Remote CI checks correctness, not these latency
ratios.

| Total panes | Final complete p50 / ImGui, both groups | At most 2x |
| --- | --- | --- |
| 16 | 1.8111–3.1244x | 2/14 |
| 128 | 1.2285–1.4969x | 14/14 |
| 1024 | 0.8292–1.1445x | 14/14 |

The two passing small conditions are single-view resize, at 1.8153x and
1.8111x. Every other small condition misses. Across all cells ImGui remains
faster in 38 p50, 35 p95 and 27 maximum comparisons. All favorable and adverse
observations are in the [last retained comparison](2026-11-07-projection-stack-reserve/comparisons.csv).

## Direct confirmation against the original runtime

A separate final pairing compares the unchanged frozen original `ea4161e`
with the final frozen runtime. This is a direct original-to-final measurement;
incremental experiment gains are not added together. It preserves the same
full workload, timer and complete output. All 42 complete p50 and p95
comparisons improve. P50 reductions range from 78.82% to 96.06%, and p95 from
78.15% to 96.47%. Maximum improves in 41 cells and regresses in one: group 2,
16 panes/four views/edge changes from 29.041 to 39.042 µs (+34.44%). The full
maximum reduction range is -34.44% to +97.84%; the negative is retained.

| Total panes | Original complete p50 range, µs | Final complete p50 range, µs | Paired p50 reduction |
| --- | --- | --- | --- |
| 16 | 5.708–26.208 | 1.167–3.166 | 78.82–91.21% |
| 128 | 46.417–204.333 | 8.166–15.709 | 81.60–94.82% |
| 1024 | 415.958–2060.042 | 66.792–117.459 | 83.07–96.06% |

Range endpoints can belong to different cells. These final absolute values
belong to the original-runtime pairing; use the separate ImGui pairing above
for its ratios. Raw phase medians are not summed into a causal fraction or a
counterfactual lower bound. The [confirmation archive](2026-11-07-twofold-result/original-confirmation.tar.xz)
preserves 8064 raw observations and 168 complete vectors; the
[complete original-to-final table](2026-11-07-twofold-result/original-comparisons.csv)
retains the adverse maximum. The
[independent confirmation review](2026-11-07-twofold-result/independent-original-review.md)
recomputed the raw observations and vectors and found no blocking issue.

## Architecture retained

The common checked Item-move path now owns local repair rather than repeatedly
normalizing and proving unrelated roots. Exact issuance and current-reference
checks preserve stale same-version rejection. General import, builder and
staged-batch paths retain their broader validation and atomic rollback.

Candidates clone an independent generational node directory and share unchanged
immutable Node/MRU payloads, root topology and placement metadata. MRU lives in
its Tabs payload. This removes deep payload and duplicate metadata work while
preserving separately prepared candidates and unchanged originals. Cloning the
SlotMap of Arc handles is still real remaining work.

Fresh layout uses shared graph-owned topology and dense root-local values,
unified measurement/projection records, packed complete parent-edge outcomes,
compact continuations and binary local input/measurement records. The existing
numeric executor and slice composers remain the authorities for numerical
behavior. The only exact-total reuse applies when its checked scalar is
unchanged. All node geometry, composed constraints, split minima/maxima/actual
extents, following gaps and overflow/unallocated outcomes remain materialized.

Complete export walks the owned result directly. Outputs remain readable after
the workspace changes or is dropped. Internal and in-crate test callers use one
`NodeResult` owner and immutable result containers; see the
[migration guide](../ENGINE_FIRST_MIGRATION.md#internal-complete-layout-results).

Each retained slice has its own ordinary semantic qualification and paired
measurements. Some ownership simplifications have mixed or adverse medians;
they are not presented as uniform speedups. The
[execution plan](../plans/2026-11-07-001-docking-twofold.md) links the complete
retained and rejected history, including forecasts that were only qualitative.
No new measured forecast is reconstructed from the final outcome.

## Rejected remaining alternatives

| Treatment | Actual control | Complete p50 improve / regress / tie | Decision evidence |
| --- | --- | --- | --- |
| Child extent slab | `e3c5171` | 1 / 40 / 1 | [Reject](2026-11-07-extent-slab.md) |
| Direct child-row composition | `e3c5171` | 0 / 41 / 1 | [Reject](2026-11-07-child-rows.md) |
| SmallVec16 continuation | `eb94cb2` | 0 / 42 / 0 | [Reject](2026-11-07-smallvec-stack.md) |
| Fused continuation | `eb94cb2` | 5 / 36 / 1 | [Reject](2026-11-07-fused-continuation.md) |
| Ordinary numeric inline | `eb94cb2` | 18 / 19 / 5 | [Reject](2026-11-07-numeric-inline.md) |
| Committed-axis validation | `07f566a` | 1 / 41 / 0 | [Reject](2026-11-07-axis-inputs.md) |
| Committed-axis/two hints, packed | `37e388f` | 0 / 42 / 0 | [Reject](2026-11-07-axis-inline-packed.md) |
| Persistent NodeStore | `37e388f` | 0 / 42 / 0 | [Reject](2026-11-07-persistent-store.md) |

Controls differ; percentages cannot be combined. Each treatment passed ordinary
semantic checks and full state equality before its performance rejection.
Persistent storage removed the N-handle fork traversal but regressed every
complete median by 67.75–151.02%. Committed-axis/two-inline specialization
regressed all medians by 2.34–25.54%. These are measured tradeoffs for those
implementations, without a universal claim about persistent storage or checks.
The original standard Vec, full validator and canonical SlotMap owner remain.

## Current diagnosis and continuation decision

The [source-matched diagnostic](2026-11-07-small-projection-diagnostic.md)
locates an initial continuation-stack expansion within timed projection.
The first short attempt finished too quickly and remains explicitly inadequate.
The independent longer probe changes only its iteration-cap assertion, restores
all 324 formal source hashes before running and obtains 2905/2941 worker
observations. Call ancestry excludes fixture and postvalidation; disassembly
identifies the exact 48-byte Enter-frame growth site. These are location
observations, without a time share, speedup prediction or lower bound.

That finding justified a bounded standard-Vec capacity reservation experiment.
It changes only the initial stack capacity, preserving ordinary deep growth,
fresh outputs and numerical behavior. It passes all ordinary gates and complete
comparisons, meeting its predeclared rules with 28 median improvements, twelve
regressions and two ties. Both selected diagnostic cases improve layout p50 in
both groups. The [full experiment](2026-11-07-projection-stack-reserve.md) retains
all negative observations. The long profile belongs to its exact `49baba1`
control; no claim is made to have profiled the final reservation executable.
Both [independent review one](2026-11-07-twofold-result/independent-stopping-review.md)
and [review two](2026-11-07-twofold-result/second-stopping-review.md) recommend
ending this round. They checked the final source, qualification, complete
comparisons, failed architectural tradeoffs and current diagnosis separately.
No additional concrete structural deletion is supported strongly enough to
require another implementation. The [decision](2026-11-07-twofold-result/decision.json)
preserves the numerical miss and final CI obligation.

Remaining source costs include the independent SlotMap candidate directory,
checked command capture and mutation, fresh leaf input, bottom-up constraint
composition, top-down constrained layout and complete result/export ownership.
ImGui updates existing mutable geometry in place; Rust additionally materializes
rich constraint/split outcomes and preserves independent prepared candidates.
ImGui also performs actual root update, normalization and visibility checks.
These scope differences do not assign a measured time share or establish a
latency floor. The retained tests reject weakening stale-reference, staged
rollback, MRU, import, numerical-error or old-output contracts.

Cross-root scratch reuse and other directory representations remain future
hypotheses rather than unmeasured promised wins. Reopen with a new source-bound
observation of specific avoidable work and a bounded complete-operation
experiment. Repeating capacity, inline or container variants without that
evidence is not required by this result.

## Verification and native CI

The final runtime passes all twelve ordinary gates with normal work counters,
including 2073 production-profile core tests, 2249 workspace tests, four public
integration tests, 292/335 independent-host tests and ten documentation tests.
The targeted set has 432 passes. Formatting, strict docs, dependency boundaries
and correctness/suspicious Clippy checks pass. The 21-by-20 work ledger matches
its actual control byte for byte. The last retained pairings preserve 16128 raw
observations and 336 complete state vectors, with an
[independent full evidence review](2026-11-07-projection-stack-reserve/independent-evidence-review.md).

The native physical smoke now acknowledges monotonic receipts of actual
consumed PointerMoved events. A valid subsequent PointerGone can clear the
current position while the primary button and drag remain active. The smoke
retains its held-button, coordinate, window/viewport lifetime, fresh step and
fresh receipt checks. Production input routing and existing state barriers
remain intact. The exact repair `c1ed518` passes all six remote CI jobs; its
Linux physical run directly observes Move followed by Gone with no latest
position and completes drag-back, release and retirement. See the
[fix and remote evidence](2026-11-07-native-motion-receipt.md).

The publication gate is all six jobs on the pushed head: Workspace, independent
engine host, official egui consumer, both native platform compiles and the
fork-backed native coordinator with physical X11 docking. The handoff must
verify the exact head, complete job conclusions and unchanged qualified source.
Its run ID and results are recorded in the accompanying execution receipt and
handoff; the earlier repair-head run alone does not qualify later runtime.

## Accounting and reproducibility

The execution ledger is `/tmp/dockspace-imgui-twofold-20261107-ea4161e`; the
tracked plan and per-treatment archives preserve branch-portable evidence.
The stop reason is independently reviewed reasonable exhaustion of this round,
with partial numerical success. Runtime and comparison binaries, compiler
commands, work receipts and raw vectors are hash-bound in each archive.
Quantitative per-slice benefit forecasts, billing cost and a separate holdout
were not recorded and are not reconstructed from outcomes. The original-to-final
pairing establishes the cumulative result; incremental gains are not summed.


The [document review](2026-11-07-twofold-result/document-review.md) identified
one publication wording issue: a full-evidence claim required the separate
reservation audit to finish. That audit is now complete and linked above; no
measurement was repeated or changed to resolve the wording.


## Subsequent binary-axis investigation

The user authorized a further [binary-axis follow-up](2026-11-07-binary-solver-followup.md).
Both exact treatments passed ordinary/work/full-output qualification but failed
predeclared performance selection against this retained runtime. Checked-total
reuse and explicit two-child initialization were rejected. All 324 formal inputs
were restored; the result and runtime in this report remain unchanged. The
follow-up does not establish a complete binary solver's performance limit.
