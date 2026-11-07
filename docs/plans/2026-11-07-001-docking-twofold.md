# Docking operation cost toward the ImGui kernel

Start from `ea4161e`. The user authorizes breaking changes, deleting redundant
implementation, editing all existing worktree changes, and committing/pushing.
Keep the real pinned ImGui source and original complete measurement protocol.

## Target and stopping evidence

Compare 16/128/1,024 total panes over one/four logical views for resize, center,
perpendicular edge split and supported cross-view moves. The measured operation
includes request capture, checked mutation, all-root layout and fresh complete
owned export. Aim for every applicable p50 cell to be within twice ImGui in both
serial ABBA/BAAB groups. Retain full state equality, raw observations, p95/max,
negative cells and differences in supported contracts.

If the target remains unmet, do not stop merely because the remaining ratio is
large. Attribute the remaining real work, implement justified structural
improvements, and assess at least two different remaining architecture choices
without an effective gain before concluding that further work is unreasonable.
Independent review must assess both correctness and that stopping evidence.

## Initial attribution and design questions

The previous four-view center result takes about 64% in checked mutation and
25% in layout. Its real path still clones the entire workspace, materializes
root value proofs, normalizes all roots, strictly validates the complete result
and derives four item multisets. A process CPU sample includes untimed fixture
construction and independent validation; its inclusive samples cannot be
presented as percentages of the measured operation.

The following hypotheses lead to distinct interventions:

1. Arena identities already provide checked slot/generation lookup, while node
   bookkeeping repeatedly hashes those identities. Replace duplicate generic
   hash state with generation-aware arena side tables where keys are known to
   be live. Preserve sparse-key resource behavior and deterministic diagnostics.
2. Atomic publication currently uses a full deep candidate. Move invariant
   ownership into a closed mutation module and assess smaller candidates or
   copy-on-write storage. No version-only seal or untracked dirty flag may
   authorize edits to internally changed state.
3. Generic normalization independently checks conservation already owned by
   typed command deltas. Identify a single conservation owner before deleting
   scans; broader builder/import normalization must keep its own guarantees.
4. Complete layout projection uses per-node tree maps and per-split owned
   arrays. Assess compact complete result storage and a cheaper solver path
   while preserving finite bounds, constraints, central allocation, cycle/error
   behavior, complete split results and output lifetime.

Do not add a general proof framework, custom compiler-like tooling or runtime
compatibility implementation. Each change must pay for actual production work,
with all callers migrated directly when an interface changes.

## Execution and verification

Root owns serial builds/tests/measurements with one Cargo build job. Native
research/review lanes separately examine transaction design, layout design and
CI. Runtime workers receive disjoint file ownership before editing. Ordinary
tests run with counters enabled; frozen timing builds disable only the test work
ledger. Existing randomized/frozen oracles remain independent.

Run relevant nextest and integration suites, doc tests, formatting, dependency
boundaries, strict documentation and correctness/suspicious Clippy checks. Check
batch staged-state/late rollback, stale same-version references, MRU and policy,
central/orphan normalization, recovery, presentation transitions and old outputs.
Commit bounded verified increments and push the existing branch. Keep evidence
compact, reproducible and proportionate; do not build an experiment framework.

## CI

Latest run `42252031294` passes every acquired job. Its macOS job has no runner,
steps or logs and reports hosted arm64 capacity exhaustion. Retry that job first;
raising execution timeout cannot repair allocation failure. Preserve the existing
expanded native-framework and cancel-close coverage. Earlier physical smoke
failures are separate observations, not evidence that this latest run failed
because of Rust code.

## Qualified sealed-layout checkpoint

The combined local-mutation/dense-output checkpoint passes 2,035 production core and 2,211 workspace tests, focused numeric/deep regressions, integration, lint and docs gates. Both complete paired comparisons preserve all state vectors. At 1,024 panes Center is 61.66–65.93% faster than ea4161e, but still 7.95–10.20x ImGui; resize remains 4.21–4.64x. No applicable cell meets 2x. Continue with runtime structural compilation, root-reference ownership and candidate copy attribution; this is not a stopping point. See [the complete evidence](../performance/2026-11-07-sealed-layout.md).

The macOS allocation retry and expanded coverage commit aa3c99b both passed all six CI jobs. Preserve those checks and require fresh CI for subsequent runtime heads.

## Qualified owner-reference checkpoint

Exact root issuance, normative sparse placement, actual-candidate transaction
capture and linear sealed-runtime compilation pass 2,047 production core and
2,223 workspace tests plus the ordinary integration/lint/docs gates. Complete
paired comparisons preserve every state vector. Against `91f42ce`, all p50
cells improve 13.20–69.37%; one p95 and seven max cells regress. At 1,024 panes,
the complete operation remains 2.39–4.25x ImGui, and no group/cell reaches 2x.
The cold recovery scale test also remains about 1.8x slower than `91f42ce`.
This is another verified improvement, not a stopping point. See
[the evidence](../performance/2026-11-07-owner-reference.md).

Next measure fresh DFS leaf input independently, then solver aggregate reuse,
prepared-report equality sealing and complete-output traversal. Continue actual
candidate-copy and cold mutation-effect ownership work after attribution. Keep
all timed leaf materialization, complete owned export, strict numeric/error
behavior, staged atomicity and current-reference semantics.

## Fresh leaf-reader checkpoint and CI correction

The explicit DFS leaf-reader slice passes 2,224 workspace tests and ordinary
public integration/lint/docs gates. Both complete comparisons preserve all
vectors. At 1,024 panes it improves complete p50 6.74–18.52% against `ecf78ba`,
but seven medians regress, including both four-view 16-pane resize groups. The
remaining ImGui ratio is 2.25–3.66x at 1,024 panes and 4.60–8.87x at 16 panes;
no cell reaches 2x. Keep small-root fixed costs and every tail visible in
[the evidence](../performance/2026-11-07-leaf-reader.md).

Pushed-head CI exposed a real unpresented resize-output retirement regression:
the new projection ownership filter ran before root geometry bases were bound.
Fix `5089b4f` moves registration after both root loops. The original public
return-to-origin test fails first, then the complete 292/335-test no-feature and
persistence-feature host suites pass. The corrected head's Workspace, host,
Windows and macOS jobs pass; remaining jobs still require their final result.

The solver-aggregate experiment passes its exact numeric/error tests and full
workspace, but 14/42 medians regress and only 8/21 cells improve in both groups.
The 1,024-pane effect is -1.08% to +3.10%, without a stable material gain. Reject
its production patch and restore the exact leaf source; retain
[the full experiment](../performance/2026-11-07-solver-aggregate.md). This is not
evidence that remaining architecture choices are exhausted. Continue sequential
complete-output reads, prepared-report ownership, payload sharing, exact cold
effects and fewer fresh layout buffers.

The sequential complete-output checkpoint passes 78 focused and 2,226 workspace
tests. Complete paired comparisons improve all but one median against the leaf
baseline, with 15.06–30.89% improvement at 1,024 panes. Eight of 42 group/cells
now pass 2x (the large resize/edge cells), while the total large-workload ratio
is 1.74–2.96x. Every small-pane cell still exceeds 2x. Five p95 and nine max
cells regress. Keep [the complete results](../performance/2026-11-07-export-reader.md)
and continue prepared-report, payload sharing and layout allocation/structure.

Prepared-report ownership passes 84 focused, 2,227 workspace and 292/335 public
host tests. Its complete-operation effect is flat/mixed (14/42 medians regress,
1,024-pane range -0.93% to +4.37%). Retain it for immutable candidate/report
pairing and one owner of final reporting; do not count it as a material speedup.
See [the full observations](../performance/2026-11-07-prepared-report.md).
Candidate copying remains the dominant measured move cost to test next.

Shared immutable Node/MRU payloads pass 2,058 production core, 2,234 workspace
and 292/335 public host tests plus the ordinary integration/lint/docs gates.
Against `93f2c57`, complete 1,024-pane moves improve 25.39–29.55%, while resize
regresses 1.55–5.47%. Thirteen of 42 group/cells reach 2x; the remaining ratios
are 3.70–7.20x at 16 panes, 1.74–3.19x at 128 and 1.29–2.04x at 1,024. Keep
the ten negative medians, all tails and cold-path cost in
[the full evidence](../performance/2026-11-07-candidate-copy.md). Mutation now
takes about 18–19 microseconds at 1,024 panes, and fresh all-root layout remains
about 110–116 microseconds. Continue exact cold effects/initial issuance, then
independently measured output slab, compact frames, numeric preparation and
graph-owned structure; do not infer that the remaining architecture is exhausted.

Closed cold effects and one-pass initial carrier discovery pass 2,062 production
core, 2,238 workspace and 292/335 public host tests plus the ordinary gates. The
existing recovery-scale test falls to a coarse 0.008 seconds, with zero actual
fingerprint builds/visits; this is not a formal isolated cold benchmark. Retain
the cleanup with its measured hot-path costs: 26/42 p50 cells regress against
`fbc8659`, range -12.51% to +7.78%, and this ImGui campaign has 10/42 cells within
2x. Preserve [all negative observations](../performance/2026-11-07-cold-effects.md).
The feasible shared-SplitArc RootStructure candidate now has a full vertical
draft and independent review underway; prioritize it once frozen and reviewed,
then revisit the queued slab/frame/fusion and single NodeResult ownership slices.
The two duplicate field-view families in the original NodeResult proposal are
unnecessary under the breaking scope and should be replaced by one result map.

CI run `42285373560` passes five jobs and every native-coordinator step before
the final physical X11/XTest smoke, which fails on a three-second inspection
socket read. Preserve request identity and stage diagnostics, investigate the
pending-viewport paint obligation in the pinned fork, and verify a concrete fix
without extending timeouts, skipping the scenario or treating a rerun as causal
evidence. Fresh pushed-head CI remains required.

## Graph-owned structures and history checkpoint

Shared immutable root structures pass 2,240 workspace tests and the ordinary
gates. They improve 36/42 complete p50 group/cells against `80403cb`, including
roughly 25–31% for large resize/center/cross-view cases, but regress both large
single-view edge groups. Single NodeResult ownership then passes 2,241 workspace
tests; its performance is mixed, with 22 negative medians. Keep it as a simpler
single result owner without calling it a measured hot-path winner. Preserve
[root-structure evidence](../performance/2026-11-07-root-structure.md) and
[NodeResult evidence](../performance/2026-11-07-node-result.md), including their
independent evidence reviews and every negative observation.

Owning MRU directly in each Tabs payload passes 2,244 workspace tests and the
ordinary gates. All center/cross-view medians improve against `fa1d4d0`; the
complete result has 38 improvements and four regressions. The unchanged ImGui
comparison reaches 25/42 group/cells within 2x: 0/14 at 16 panes, 11/14 at 128,
and 14/14 at 1024. The remaining ranges are 2.73–5.14x, 1.54–2.08x and
1.09–1.50x, respectively. The
[full MRU evidence and independent review](../performance/2026-11-07-tabs-mru.md)
retain tails, logical copy counts and
unsuccessful size diagnostics without inventing allocator-byte attribution.

Continue independent placement/root metadata ownership and fresh layout buffer
experiments. Existing queued extent-slab, indexed child composition and compact
continuation candidates are not qualified or measured merely because their
source reviews pass. A bounded Allocations recording timed out before producing
useful samples; no allocation-time share is established by that attempt.

## Retained native input after admission retirement

The pending inspection-paint fix is pinned and pushed in `b662164`. CI run
`42297763668` passes the earlier inspection and native-framework gates, then
fails after successful Center merge, physical release and passthrough restore
with `Native(InvalidPointerFacts)` during retirement quiescence. This is a
second concrete failure and keeps CI repair open.

An extension to the existing retirement fixture preserves a real callback's
ordinal and exact child hover/capture across release suppression. Root's serial
nextest run compiles successfully and fails at the added expected replay point
with the exact `InvalidPointerFacts` chain. The child is no longer current but
remains recognized. This proves the retained-fact/current-admission mismatch;
it does not prove the precise X11 callback chronology of that CI run. Independent
review must establish the reducer's current-action gates before widening fact
admission, retain foreign/forgotten/backend/work-area rejection, and require
fresh physical smoke at the pushed corrected head.

The bounded admission fix now passes the same causal test, 340 core boundary
cases, 545 native tests, 2,245 workspace tests, 593 fork-workspace cases and 545
native tests with the native crate's debug assertions disabled, plus the lint,
format, docs and dependency gates. Exact historical journal facts are recognized;
recovery rosters retain current-binding checks and actions retain reducer-time
authority. See [the evidence](../performance/2026-11-07-pointer-retirement.md).
The original physical run's exact callback remains unknown. Fresh remote CI is
still required.

Placement sharing passes all ordinary gates and complete comparisons, with
17/42 complete medians improving, 24 regressing and one tie. Large center/cross
mutation medians fall from about 11 to 6.1 microseconds, while complete-operation
improvements there are 2.16–6.46%. Small cases remain mixed/worse, so this is a
bounded ownership benefit and not a uniform performance winner. ImGui has
26/42 cells within 2x; all fourteen 16-pane cells remain above the target. Keep
[all negatives and the independent evidence review](../performance/2026-11-07-placement-sharing.md).
Continue the reviewed Item-frontier placement-completion and fresh-layout
storage candidates; this is not a stopping point.

Item-frontier completion passes the full ordinary gates and removes actual
duplicate placement installation. Its complete pairings improve 24/42 p50
group/cells, with 17 regressions and one tie. Large single-view edges improve
12.72–12.75%, four-view edges 3.07–7.37%. All 128/1024-pane cells meet 2x in
both groups, while all fourteen 16-pane cells still exceed it: 28/42 overall.
Preserve [the complete evidence and independent review](../performance/2026-11-07-item-frontier.md).
The separate row-capacity candidate is under ordinary qualification; do not
mix it into these results.

Fresh pushed-head CI `42308166838` passes all six jobs at exact `1c71696`,
including actual output/backpressure and physical X11/XTest guide-control
docking. It crosses retirement quiescence, verifies child-window destruction
and viewport removal, and reaches successful completion. The native repair
checkpoint is verified; subsequent performance heads still need fresh CI.

## Item row reservation checkpoint

The row-capacity slice is committed and pushed as `e3c5171`. All ordinary
gates pass, and its 21-row, 20-column work CSV matches the Item-frontier
control exactly. Complete comparisons improve 27/42 p50 cells and regress
15, including both small single-view Center groups. Retain the bounded edge
benefit with every tail and negative in the [evidence and independent review](../performance/2026-11-07-row-capacity.md).
The target remains 28/42: all 128/1024 cells pass; every 16-pane cell exceeds
2x. The separate child extent slab is currently under ordinary qualification;
queued source reviews alone establish neither qualification nor performance.
Fresh CI run `42312915309` for `e3c5171` is still in progress.

The child extent slab passes all ordinary gates and preserves complete output,
but 40/42 complete p50 comparisons regress against the capacity control. Reject
its exact runtime patch and restore the two immutable control sources. The
[complete rejected experiment](../performance/2026-11-07-extent-slab.md) retains
all raw observations and negatives. Continue the other layout candidates on the
retained three-Vec base; no source-only candidate is yet qualified or measured.

Direct child-row composition also passes all ordinary gates/work and preserves
complete vectors, but 41/42 complete p50 cells regress and one ties. Reject its
exact three-source delta and retain the original gather-based composition;
[all observations and restoration identities](../performance/2026-11-07-child-rows.md)
remain available. The target is still 28/42. Compact frames, binary inputs and
committed weight validation must be rebased onto the retained control before
their independent measurements; these two rejected layouts are not stopping
evidence that remaining justified directions are exhausted.

Exact-head `e3c5171` CI run `42312915309` now passes all six jobs, including
the physical X11/XTest docking and retirement checks. The retained runtime base
is remotely green. Rejected slab/child-row experiments leave that runtime
unchanged; the active compact-frame candidate remains local and unqualified.

Compact continuation now passes all ordinary gates (2,070 production core,
2,246 workspace and 292/335 host tests) and preserves the entire work/vector
matrix. It improves 35/42 complete p50 cells against e3, regresses five and ties
two; all large cells improve 2.05–6.49%. Retain it with all adverse tails in
[the complete evidence](../performance/2026-11-07-compact-frame.md). The target
remains 28/42, with every small cell above 2x. Continue separate binary inputs,
unified values, committed weight checks and metadata ownership. New runtime
commits still require fresh remote CI.

Binary input storage passes the complete ordinary gates and preserves all
work/vectors. It improves 40/42 complete p50 cells against the compact-frame
control, with two regressions; all large group/cells improve 1.16–5.55%.
Retain this one-file change with the unchanged solver and
[all adverse observations](../performance/2026-11-07-binary-inputs.md).
The target remains 28/42, with small ratios now 2.12–4.20x in this campaign.
Continue unified values, committed weight checks and the reviewed concrete
command ownership deletions. Compact-frame CI `42318019349` is still running.

Unified measurement/projection values pass all ordinary gates (2,072 production
core, 2,248 workspace and 292/335 host tests) and complete work/vector equality.
Against binary inputs, 39/42 complete medians improve, two regress and one ties.
Retain the fresh shared owner and temporary scalar central route with
[all adverse cases and initialization costs](../performance/2026-11-07-unified-values.md).
The total is still 28/42; all small cells remain above 2x. Continue the
committed/no-override weight-check boundary and reviewed command ownership
combination before evaluating more invasive output/directory alternatives.

The narrower committed-axis validation candidate passes all ordinary gates
and complete work/vector equality, but 41/42 complete medians regress against
the unified control, including every large cell by 11.75–24.47%. Reject and
restore its exact two-source delta; preserve the [full evidence](../performance/2026-11-07-axis-inputs.md).
Keep the original full validator and continue the reviewed command ownership
combination. Rebase the complete parent-edge output candidate onto this
retained checked parent; its source review alone is not executed evidence.

Concrete command ownership passes every ordinary gate and complete work/vector
equality. Retain the bounded ownership simplification with mixed complete
medians: 21 improvements and 21 regressions, while mutation improves in 22/30
command cells. Keep all negative cells/tails in the [evidence](../performance/2026-11-07-command-ownership.md).
The target remains 28/42. Continue the independently reviewed parent-edge
result-storage experiment against this actual retained control, then the
bounded continuation stack and source-motivated compiler-boundary follow-up.

Materialized parent-edge projection results pass all ordinary gates and the
complete work/vector matrix. Retain the representation with 33/42 complete
p50 improvements, five regressions and four ties; all adverse tails and wider
record/scratch/extent-copy costs remain in the [evidence](../performance/2026-11-07-packed-results.md).
The target remains 28/42. Next measure the separately reviewed SmallVec
continuation, fused control transitions and checked-axis compiler-boundary
follow-up. A fresh unified-head physical drag-back CI failure is being traced
on its immutable head; do not confuse its failure with unpushed output WIP.

The standalone SmallVec16 continuation passes all ordinary/work/vector gates
but regresses every complete p50 against packed results, by 2.52–33.01%.
Reject its exact eight-source/configuration patch and preserve the
[complete failed experiment](../performance/2026-11-07-smallvec-stack.md).
The retained target remains 28/42. Rebase fused transitions onto the standard
Vec and packed output parent; the SmallVec composition is not retained.

The standalone fused standard-Vec transitions pass all ordinary/work/vector
gates but regress 36/42 complete medians against packed results, including
every large cell. Reject its exact one-file delta and preserve the
[complete failed experiment](../performance/2026-11-07-fused-continuation.md).
The retained target remains 28/42. Continue the independently reviewed
numeric, axis, persistent-directory and binary-measurement candidates.

The standalone ordinary numeric inline hint passes all ordinary/work/vector
gates, but the complete median mix is 18 improvements, 19 regressions and
five ties, including eleven large regressions. Reject the exact one-line
delta and preserve [the complete evidence](../performance/2026-11-07-numeric-inline.md).
The retained result remains 28/42; continue actual redundant-total reuse and
the remaining independently reviewed architecture candidates.
