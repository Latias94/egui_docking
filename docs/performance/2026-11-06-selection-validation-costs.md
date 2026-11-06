# Selection, validation and normalization costs

Date: 2026-11-06. Production baseline:
`eea8a7f8798187214b6fd086585906db67c99036`.
This follows the [compiled-layout experiment](2026-11-06-compiled-layout.md).

## Finding

Full validation and normalization are substantial costs on the current tab
selection path. A change to one leaf's selection/MRU still enters the general
structural transaction and subsequently rebuilds and validates the surface
scene. The evidence supports reducing the scope and repetition of those checks;
it does not support disabling runtime validation wholesale.

The current measurements do not identify cloning as the dominant remaining
cost. The workspace snapshot and requirement inventory already share immutable
ownership. Replacing ownership indiscriminately would miss the larger repeated
traversals and materialization work.

Temporary diagnostic source changes were removed by verifying their exact
instrumented hashes and restoring their saved original bytes. All 12 original
source hashes matched the baseline before the separate minimal production change
described below. That change preserves the existing validation semantics.

## Diagnostic workload and boundaries

The existing public, renderer-free `frame_cost` example is unchanged apart from
temporary timing spans. It still measures, compiles, records every ordered paint
operation, accepts its receipt, commits the host frame and settles the output.
Selection alternates the two items of one tabs leaf. It is not the product
benchmark's two-frame physical click.

The diagnostic uses the existing release ThinLTO/one-codegen-unit settings, one
Cargo job, 32 samples and six warmups at 16/128/1,024 leaves. Timing and active
heap profiling are separate; these diagnostic runs use inactive DHAT. Explicit
spans cover public calls and their internal stages. Complete workload assertions
pass. The logging and changed compilation can perturb timing, especially at the
small scales; this is cost attribution, not a new latency qualification.

Each table below reports independently measured stage medians. Nested stages
are identified explicitly and must not be added to their parents. Summing stage
medians does not produce a percentile of total frame time.

At 1,024 leaves, the 32 diagnostic selection updates have these coarse costs:

| Sequential stage | p50 |
| --- | ---: |
| Begin rollback frame | 0.034 ms |
| Submit selection | 3.797 ms |
| Measure and produce the new scene | 8.493 ms |
| Obtain paint work | 0.002 ms |
| Encode the complete host recording | 1.032 ms |
| Confirm paint | 0.001 ms |
| Commit | 0.546 ms |
| Complete instrumented update | 14.004 ms |

The host recording is intentionally retained. Its approximately 1 ms is not
all a docking-core cost and cannot be removed while claiming the same workload.

## Transaction cost is mostly surrounding the edit

| Inside the transaction, nested in selection submission | p50 |
| --- | ---: |
| Clone workspace draft | 0.053 ms |
| Initial item ledger | 0.072 ms |
| Apply commands, including source verification | 0.199 ms |
| Prune vacant surfaces and canonicalize | 1.244 ms |
| Explicit workspace validation after canonicalization | 0.420 ms |
| Final item ledger | 0.060 ms |
| Complete instrumented transaction | 2.173 ms |

Inside `select`, source validation takes 0.172 ms. The actual selected-field/MRU
edit takes approximately 0.000167 ms (167 ns). The latter is close to timer
resolution and should not be treated as a precise microbenchmark. It does show
that the scalar edit is not the millisecond-sized operation. Publishing the
prepared transaction's equality-based report takes approximately 0.000375 ms
in this workload; it is not a demonstrated dominant cost either.

The amplification is visible directly in the implementation:

1. `operation.rs::prepare_transaction_inner` clones the whole workspace and
   creates its item multiset before applying the command.
2. `operation.rs::select` checks its source, assigns one selection and promotes
   the selected item in that leaf's MRU. Its declared item delta is `None`.
3. `canonical.rs::canonicalize_draft` validates every node's selection, MRU,
   weights, child references and unique ownership, then checks cycles and roots.
4. `canonicalize_validated_draft` captures item multisets before and after,
   rewrites the forest, computes reachability, sweeps unreachable nodes, and
   calls `Workspace::validate_candidate`.
5. `graph.rs::validate_candidate` delegates to the complete workspace validator.
6. Back in `prepare_transaction_inner`, `candidate.validate()` invokes that
   complete validator again, followed by another item reconciliation.

These checks support a general structural-edit operation. A selection edit on
an already validated immutable snapshot cannot create an edge or cycle, remove
an item, change a split weight or produce an empty structural container.
Re-establishing all those properties is avoidable if the selection operation's
private interface carries the necessary validated-snapshot provenance and
preserves the unchanged invariants by construction.

That qualification matters. Simply deleting validation from the shared generic
transaction would change its behavior on malformed drafts, structural commands
and ordered batches. The low-level source fingerprint and stale-input semantics
also cannot be bypassed merely because an item ID still exists.

### Minimal production change: consume the existing postcondition

Four callers perform a complete `validate()` immediately after successful
`canonicalize_draft()`, with no intervening workspace mutation:

- `prepare_surface_content_close`;
- `prepare_content_close`;
- `prepare_transaction_inner`;
- `apply_owned_draft_command`.

The change removes those four duplicate calls and explicitly documents
`canonicalize_draft`'s already existing validated-output postcondition. It keeps
the full strict validator inside canonicalization, all source checks, input
validation, canonical rewriting, MRU checks and item reconciliation. No new
cache, trusted flag, feature, public interface or specialized transaction is
introduced. If the candidate is invalid, the same canonicalization validation
already returns the error before any removed call could execute.

This does not implement the larger local-selection or checked-scene proposals.
It removes one exact repetition first so its benefit can be qualified separately.

### Qualification of the minimal change

The production candidate was built after every diagnostic source change had
been removed. Its source inventory differs from the frozen `eea8a7f` product
baseline only in `operation.rs` and the `canonicalize_draft` documentation. Both
workload sources and ImGui inputs are byte-identical. Formal timing runs do not
overlap compilation, tests, diagnostic tracing or independent reviews.

The unchanged renderer-free workload ran in one ABBA and one BAAB order group,
48 samples and six warmups per process, at all three scales. At 1,024 leaves:

| Update scenario | Four before process p50 values | Four after process p50 values |
| --- | --- | --- |
| Select one tab | 13.529, 13.497, 13.483, 13.514 ms | 13.104, 13.133, 13.152, 13.258 ms |
| Idle | 1.855, 1.859, 1.862, 1.861 ms | 1.862, 1.876, 1.867, 1.860 ms |
| Change measurements | 12.318, 12.365, 12.289, 12.344 ms | 12.444, 12.439, 12.433, 12.439 ms |

Selection is consistently faster in these processes. Idle and changed-measurement
paths have identical allocation counts/bytes before and after; they do not take
the removed selection validator. Their timing variations are retained here and
are not used to claim a speedup outside the modified transaction paths. The
small changed-measurement timing slowdown has not been given a causal function
attribution. Tail results and all other cases remain in the archive.

Separate active-DHAT runs use six samples and two warmups. Selection update
allocation events fall from 109,009 to 103,852: **5,157 fewer events (4.7%)**.
Requested bytes fall by 584,200, from 54,195,152 to 53,610,952. Settlement counts
and bytes remain unchanged. The retained host recording and complete output
workload are unchanged. Requested bytes are aggregate allocation requests, not
resident heap or RSS.

The complete fork product ran the original 24-sample, six-warmup, three-scale
workload, first through the existing Rust/ImGui script and then using frozen
before/after Rust executables in one ABBA and one BAAB group. At 1,024 panes:

| Scenario | Group 1 before → after p50 | Group 2 before → after p50 |
| --- | ---: | ---: |
| Idle, one frame | 8.388 → 8.280 ms | 8.373 → 8.157 ms |
| Physical tab click, two frames | 31.246 → 30.553 ms | 30.403 → 30.215 ms |
| Surface resize, one frame | 19.167 → 19.071 ms | 18.603 → 18.799 ms |

The click medians improve by 0.6–2.2% within these groups. The idle improvement
does not establish a direct benefit from removing a validator that idle does
not call; code generation and machine state also change timing. No broad
idle/resize or tail-latency improvement is qualified. This small change is
retained for eliminating exact duplicated work and the independently observed
selection allocation/timing reduction.

All 2,151 workspace tests, 292 default-host tests, 335 serde-host tests, 556
native tests and 10 doctests passed. The normal suites skip the opt-in layout
probe and one pre-existing native wall-time case. Workspace/native
correctness/suspicious Clippy, formatting, dependency-boundary and whitespace
checks passed. Independent read-only review found no blocking issue in the
four-call removal and confirmed unchanged validation, error and rollback
semantics.

## Scene creation repeats other structural work

| Internal stage during one selection update | p50 | Nesting |
| --- | ---: | --- |
| Requirements refresh, three calls combined per update | 1.519 ms | Across submission/commit; includes the next three rows |
| Surface semantic snapshots, three calls combined | 0.417 ms | Inside requirements refresh |
| Build workspace index, once | 0.138 ms | Inside requirements refresh |
| Derive requirement draft, once | 0.419 ms | Inside requirements refresh |
| Compile surface scene, once | 3.570 ms | Inside measurement contribution preparation |
| Validate/canonicalize scene, once | 3.305 ms | Inside measurement contribution preparation |
| Install scene, once | 0.741 ms | Inside measurement contribution compilation |

The requirements refreshes do not all rebuild the index: the observed full index
and draft derivation occur once, while all three refresh calls capture surface
snapshots. `capture_surface_semantic_snapshots` walks roots and captures their
fingerprints before the unchanged-roster early return. Fingerprints include
selection/MRU, and a fresh requirement index starts without the previous compiled
layout topology. Sharing only the compiled topology cannot remove this work.

`engine/surface_contribution.rs` compiles measured facts, then invokes
`PresentationPlanValidator::validate_and_canonicalize`. The latter constructs
semantic uniqueness indexes, derives expected pane records from the workspace,
joins pane/tab/control records, checks targets and layers, canonicalizes and
links the executable program. The approximately 3.3 ms is not just assertions:
it includes result preparation needed by downstream consumers. Treat it as an
opportunity to construct and prove shared rows once, not as 3.3 ms that can be
deleted by returning the raw plan.

Within those scene-validation spans, p50 is 0.466 ms for uniqueness indexes,
1.253 ms for the semantic-check section, 0.962 ms for the target-check section
and 0.163 ms for canonicalization. These are nested checkpoints; the final
program linking and other work are included in the parent span.

Using each frame's own measurements, the disjoint transaction, requirements
refresh, scene compilation, scene validation and scene installation spans
occupy an average 81.35% of its instrumented selection update (79.28–82.65%
across 32 samples). This calculation excludes their nested snapshot/index spans
and is not a sum of medians or a predicted removable percentage.

### Existing geometry reuse remains relevant

On selection, hit-manifest work takes about 0.265 ms, paint-program validation
0.229 ms and semantic-manifest creation 0.195 ms. In the measurement-change
case, scene installation takes 3.801 ms, of which hit-manifest work is 3.252 ms.
These span boundaries do not separately time touch-fragment construction.
The implementation first tries exact hit-geometry reuse, so a new output does
not imply rebuilding touch geometry. A lazy or incremental redesign must account
for the existing hit and miss paths rather than describing them as unconditional
recomputation.

## What the pinned ImGui implementation does

The comparison reference is Dear ImGui commit
`d88f238d1d363ef387382b2e29ad12cf96f826ac`, matching the previous benchmarks.
The observations below concern that exact source, not every ImGui release.

ImGui has structural maintenance and checks. [`DockNodeUpdateFlagsAndCollapse`](https://github.com/ocornut/imgui/blob/d88f238d1d363ef387382b2e29ad12cf96f826ac/imgui.cpp#L19260)
recursively visits children, removes inactive/closed windows, collapses nodes,
propagates flags and updates visibility. `DockNodeUpdateForRootNode` then
collects central-node and visible-window information. `DockNodeTreeSplit` and
`DockNodeTreeMerge` directly maintain child and parent relationships. It would
be incorrect to say that ImGui never traverses its tree or normalizes structure.

Tab selection is applied to the existing tab bar's selection fields in
[`TabBarLayout`](https://github.com/ocornut/imgui/blob/d88f238d1d363ef387382b2e29ad12cf96f826ac/imgui_widgets.cpp#L10165); `DockNodeUpdateTabBar` maintains visibility
and focus. It does not add a cloned whole-workspace transaction with a general
forest rewrite and item-multiset reconciliation for that local selection.
Normal docking maintenance and other per-frame UI work still happen.

Many programmer-invariant checks use [`IM_ASSERT`](https://github.com/ocornut/imgui/blob/d88f238d1d363ef387382b2e29ad12cf96f826ac/imgui.h#L95), which defaults to C/C++
`assert` in the pinned configuration. The existing benchmark builds ImGui with
`-DNDEBUG`, so those default assertions are compiled out. This is a material
contract difference from our always-on recoverable graph/scene validation.
However, `IM_ASSERT_USER_ERROR` macros also perform runtime error dispatch, and
normal state-maintenance branches remain present. Assertions being disabled
does not mean all ImGui validation or maintenance vanishes.

### Assertion-switch experiment

An additional build removes only `-DNDEBUG` from the original pinned ImGui
production command. Compiler version, source hashes, `-O3` and ThinLTO match
the original. Frozen assertions-disabled/enabled executables run in one ABBA
and one BAAB group, with 128 samples and 16 warmups at 1,024 panes. All frame,
pass, content and mesh counts are identical for every corresponding sample.

| Complete ImGui scenario | Assertions disabled p50, groups 1/2 | Assertions enabled p50, groups 1/2 |
| --- | ---: | ---: |
| Idle | 1.320 / 1.312 ms | 1.106 / 1.114 ms |
| Tab click, two frames | 2.263 / 2.265 ms | 2.216 / 2.221 ms |
| Surface resize | 1.296 / 1.297 ms | 1.111 / 1.116 ms |

The enabled build happens to be faster in this experiment. Enabling assertions
changes optimization and code layout as well as executable checks; these values
are not an isolated price per assertion or a general recommendation about build
flags. They show that this unchanged docking workload remains in the same
millisecond range with assertions enabled. Disabled assertions do not, in this
experiment, account for the large difference from our current full-runtime
verification and scene-reconstruction path. Neither build implements our full
recoverable workspace/scene validation contract, so this is not a validation
feature-parity comparison.

## Idle-regression status

The previous experiment recorded 1.4–2.7% slower complete-product idle medians
in four interleaved groups and retained that limitation. This investigation
reuses exactly the same frozen old/new executables and their hashes:

| New comparison | Before/after idle p50 change, group 1 | Group 2 |
| --- | ---: | ---: |
| Only 1,024 panes; 48 samples, 12 warmups | +0.185% | +0.767% |
| Original 16/128/1,024 sequence; 24 samples, six warmups | +1.169% | −0.260% |

Each comparison contains one ABBA and one BAAB order group. The first comparison
changes several workload parameters and is only a narrower diagnostic, not an
isolated test of warmup or scenario-order effects. The second restores the exact
original workload parameters. Both retain all samples. The prior idle regression
is not stable enough across these runs to support a function-level causal claim
or a speculative fix. This does not prove a small regression is absent.

## Refactoring direction

Runtime validation should follow the trust and mutation boundaries. Keep full
validation for loading or constructing arbitrary layouts, accepting independent
measurements, crossing host/provider/output identities, and applying structural
changes whose affected invariants are not otherwise established. Keep stale
source/output rejection and atomic rollback in all paths.

For the internal hot path, pursue these bounded modules rather than adding a
parallel public protocol:

| Candidate | Intended change | Required proof | Scope/risk |
| --- | --- | --- | --- |
| Local selection preparation | Reuse the existing checked transaction result and publication fences; apply one validated selection/MRU delta without a general topology rewrite | Current immutable snapshot/source, exact item membership, exact MRU, ordered command semantics and discard-on-failure | Smallest first implementation if restricted to the appropriate private caller; retain the cheap workspace clone initially |
| Structural identity independent of selection | Share structural rows and indexes across selection changes; refresh geometry only when actual measurements or constraints require it | Generational node/edge/weight/central provenance independent of selection and output authority | Broader model change; measurement and historical-output behavior need explicit tests |
| Checked scene construction | Produce checked shared record rows and executable program linkage together; reuse those proofs in private consumers | Complete coverage, uniqueness, geometry/layer rules, measurement provenance and program executability bound to the exact plan | Largest measured stage opportunity, but cannot be implemented as a validator bypass or a forgeable boolean flag |

The recommended order after the exact duplicate removal is local selection,
structural/semantic reuse, then incremental checked-scene construction. Begin
with a single Select whose base is an exact known canonical publication. A
workspace index alone is not proof of complete canonical validity. Unknown
drafts and mixed batches retain the existing generic transaction. Keep the
inexpensive discardable workspace clone at first; avoiding repeated global
reasoning is the first objective, rather than simultaneously changing storage
and publication.

The local path must still preserve stale-source rejection, MRU ordering, no-op
reporting and focus requests on reselection. Pointer/menu selection may also
raise a contained window or close a popup through the existing journal; those
compositions cannot be reduced to a bare selected-field setter. Both the direct
product command and journal paths must use their established publication fences.

For structural reuse, preserve the existing complete command fingerprints:
selection legitimately makes old commands stale. Introduce narrower private
dependencies only for reusable computation. Selection can change the selected
pane's policy or measured minimum, so unchanged topology does not authorize
blind geometry reuse. Keep exact allocation/version provenance, same-version
mutation defenses and native counterfactual projections in the existing index
and presentation owner instead of adding parallel caches.

For scene work, extend the existing typed rows, linked immutable surface program
and private prepared contribution. Construct their shared inventories and proofs
together. Final canonical ordering must precede row linking, or rows need an
explicit final remap. A compiler-origin flag cannot substitute for the remaining
coverage, geometry, policy or executability obligations.

None of the measured stage costs is a guaranteed future saving. Preserve the
existing generic validator as an oracle while developing a smaller checked path,
and verify equivalence for stale sources, missing items, MRU, repeated selection,
multi-command rollback, measurement-dependent minima, native projections and
historical outputs. The next implementation should target one of these modules
and qualify it on the unchanged full frame and product workloads.

The [evidence archive](2026-11-06-selection-validation/README.md) retains raw
measurements, diagnostic spans and their exact instrumentation patch, independent
review, source/binary hashes, qualification results and all gate logs. Its
verification script recomputes every exposed raw-sample percentile and checks
complete coverage and equal product work. No favorable subset replaces the
recorded idle or unchanged-path results.
