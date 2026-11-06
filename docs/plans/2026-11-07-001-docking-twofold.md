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
about 110–113 microseconds. Continue exact cold effects/initial issuance, then
independently measured output slab, compact frames, numeric preparation and
graph-owned structure; do not infer that the remaining architecture is exhausted.
