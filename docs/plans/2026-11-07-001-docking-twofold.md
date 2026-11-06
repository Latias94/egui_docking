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
