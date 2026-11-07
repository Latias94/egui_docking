# Independent challenge of the remaining small-root gap

Read-only analysis, November 7, 2026. Parent is immutable
`eb94cb242722962c9e0d88074d81cf462d784357`, the retained packed-result
Implementation. Only this report was written. No repository/fork/cache or
candidate change, Cargo, compiler, test, runner, timing, profiler, network or
child agent was used. No independent MRU report was read.

**Verdict:** the retained result does not meet the all-cell twofold goal, and
the available evidence does not justify declaring it unattainable. I did not
find another independently attributed, low-risk architecture change that
should jump ahead of the existing queue. One bounded owner-lifetime hypothesis
remains below, but its benefit is unmeasured, it primarily concerns multiple
roots, and it does not supply a solution for the remaining single-root cells.
Preparing another container candidate now would outrun the evidence.

## What the current measurements actually establish

The packed frozen source receipt matches eb94cb2 for layout.rs, projection.rs,
topology.rs, docking_cost.rs, operation.rs, shared_storage.rs and the actual
ImGui wrapper. Its candidate binary is
`21888a3b2905da0edce33f68d4a7e3dbf68ba0e45d65f1d72df5d4d03f110435`;
the comparison uses pinned ImGui
`d88f238d1d363ef387382b2e29ad12cf96f826ac`.

I read the saved environment and summary, not a newly executed benchmark.
There are serial ABBA/BAAB groups, 48 samples/eight warmups per process/cell,
96 measured observations per condition/group/cell. The environment records
complete-state equality and unchanged frozen executables. The report's
28/42 successful group/cells is reproduced from those summaries: all fourteen
128-pane and all fourteen 1024-pane cells meet twofold; none of the fourteen
16-pane cells does.

The table gives the range of each independently recorded phase median across
the two groups, in nanoseconds. Columns are deliberately not added or used
as counterfactual bounds.

| Views / case, 16 panes | Request | Mutation | Layout | Export | Complete Rust | Complete ImGui | Complete ratio |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 / resize | 0 | 0–41 | 1,042 | 209 | 1,292 | 625 | 2.067 |
| 1 / center | 83–84 | 708–709 | 1,041–1,042 | 209–250 | 2,042 | 667 | 3.061 |
| 1 / edge | 83–84 | 1,667–1,708 | 1,167–1,208 | 250 | 3,167–3,250 | 1,042–1,083 | 2.924–3.119 |
| 4 / resize | 0 | 0–41 | 1,291 | 209–250 | 1,541–1,542 | 583–584 | 2.639–2.645 |
| 4 / center | 83–84 | 667 | 1,291 | 209 | 2,250 | 625–667 | 3.373–3.600 |
| 4 / edge | 83–84 | 1,500–1,542 | 1,417–1,458 | 250 | 3,291–3,375 | 917–959 | 3.432–3.680 |
| 4 / cross-view | 84–125 | 708–750 | 1,291–1,333 | 208–250 | 2,292–2,417 | 625 | 3.667–3.867 |

Very small phase observations approach clock quantization; a zero recorded
median is not zero physical work. Layout is a material residual, but mutation
is larger in the one-view edge case and comparable in the four-view edge case.
“Layout dominates every small operation” is not supported. Export medians are
already below the native common exporter here; that does not prove optimality.

## The available profiles do not partition this retained binary

| Saved diagnostic | Actual scope | What it cannot establish |
| --- | --- | --- |
| `B/layout-hotspot` | Repeated fresh 16-pane layout at one/four views, a2365aa/07f566a-era source; full projection disposal in the loop. Root's evaluation binds those older source hashes. | Current packed solver/allocation costs, request/mutation/export costs, formal phase shares. |
| `B/profile-center.sample.txt` and attribution | Earlier 1024-pane one-view center probe. The saved CSV confirms that cell. The process includes fixture/validation; the narrower prepare attribution is still from the superseded transaction Implementation. | Current 16-pane costs or persistence of deleted global proof/canonical work. |
| `B/root-structure-profile` | Earlier 1024-pane edge, sampled process with fixture/validation; only explicitly selected mutation ancestry is meaningful. | Small-root current phase or allocator fractions. |
| `B/small-root-allocations` | Attempted 16-pane four-view resize allocation trace, terminated after a 45-second timeout. | Any valid allocator count, byte total, time share or successful trace conclusion. |

The old 16-pane layout sample has selected project_measurements/grow_one frames
with 266/2,945 main-thread samples at one view and 217/2,887 at four views.
Those are frame counts, not exclusive allocation percentages. Its driver
drops the full returned projection Vec at each statement; formal timing keeps
rich projections alive through the endpoint. Packed output changed the
Implementation after that sample. No current, clean four-phase 16-pane
profile was found in the supplied evidence.

SmallVec provides actual negative evidence rather than a hypothetical fix:
its retained-control complete p50 regresses in all 42 group/cells, although
ordinary semantics/work qualify. That rejects that exact container treatment.
It neither identifies the exclusive reason nor proves that temporary owner
cost cannot be reduced by another design. Other rejected slab/child-row
treatments likewise do not turn source owner counts into a lower bound.

## Necessary work versus repeated Implementation work

The existing Module already earns useful Depth: current source/target/policy
checks and one independent candidate stay behind the transaction Interface;
local completion does not re-enter the former global settlement path.
The current source does not support resurrecting those obsolete bottlenecks
as an explanation.

| Phase | Required observable work | Actual residual or repetition |
| --- | --- | --- |
| Request, `docking_cost.rs:136–173` | Resolve actual current leaves, capture independently checked source/target and construct the real command. | Root/membership lookups and owned captures remain; successful verification already borrows issuance (`workspace.rs:743–789`). Tiny medians do not support making this the primary bottleneck. |
| Mutation, `operation.rs:629–705`, `:1201`, `:1759` | One checked executor, current policy, selected/MRU semantics, atomic discardable candidate and owned report; edge repair/current RootStructure. | NodeStore copies all live handles; changed payloads COW; root issuance allocates; a structural edge builds current rows. Persistent-directory and other queued treatments already address the principal identified mechanisms. |
| Layout, `topology.rs:133`, `layout.rs:598`, `:953` | Every fresh leaf answer, deterministic composed constraints, actual bounded allocation, every node rectangle/edge outcome/gap and typed error order. | Gather/scratch owner setup, default result writes, repeated numeric scans and continuation bookkeeping are Implementation choices. The known queued/rejected treatments cover several, not every conceivable representation. |
| Export, `docking_cost.rs:277–351` | Fresh complete vector, actual semantic members/selection, all-root geometry and identity join. | Already uses projection preorder; it does not rebuild an index per node. No supported reason to replace it with a digest, cached vector or geometry-only partial result. |

Rust returns more than the common vector. The three owned buffers hold
NodeResult rectangles plus composed min/max, per-split outcomes, and
ChildProjection min/max/actual extent/following gaps. They keep old values
after Workspace mutation/destruction and support independent Clone.
ImGui's selected kernel writes existing node Pos/Size directly from native
SizeRef/style and does not return these fresh rich results. It also performs
all-root flags/collapse/FindInfo during mutation and a local edge layout during
split, before the explicit layout phase. The comparison is legitimate under
its stated contract, but its phase partition is not symmetric feature parity.

Postorder constraints and preorder bounds have real data dependencies.
Allocating a parent needs all child subtree constraints; child bounds need
the parent's allocation. Early subtree measurement can fail before a later
leaf is requested, and all measurement precedes geometry allocation. Merging
the walks without preserving those events changes first errors. Fresh typed
fields can still overflow when composed or added to geometry, so aggregate
and endpoint checks are not universally redundant.

These obligations require reading/materializing information. They do not
require today's exact Vec growth, Arc count, dispatch or machine instructions.
Neither “three buffers” nor “two traversals” proves a wall-clock minimum.
Rust's ownership rules are not an independent explanation of the measured
ratio; this concrete immutable output/candidate contract is the relevant
choice.

## One remaining owner-lifetime hypothesis, not another ready candidate

There is source evidence for repeated root-local temporary owners in a
multi-root operation:

- Production measurement creates `child_values = Vec::new()` for every root
  (`topology.rs:148`), gathers children in order and drops it on return.
- Each projection creates a new continuation Vec and ProjectionScratch
  (`layout.rs:966–967`). Binary input arrays avoid most generic scratch
  population, but do not remove the continuation owner.
- `project_all_roots` calls this whole sequence separately for each root
  (`docking_cost.rs:215–256`). The scene surface compiler also sequences
  complete main/contained roots, with delayed minimum measurement.

A concrete scratch owner scoped to **one actual all-root/surface operation**
could retain only the child-gather and continuation capacities between roots.
Every result still receives its own three fresh buffers; every leaf is read,
every arithmetic/check/write remains in place, and scratch is destroyed before
the original layout endpoint. No frame-to-frame cache, prewarmed missing
metric, global mutable state, new container, provider or trust bit is needed.
This differs from SmallVec's inline storage and from child-row copy removal:
the same gathers and dynamic Vec events remain, with a different owner lifetime.

The design must use one executor behind a narrow internal Seam, with the
existing scene compiler and headless all-root Adapter as actual consumers.
Adding a benchmark-only scratch executor would not be acceptable. Public
single-root convenience calls can create the same local scratch internally;
delayed RootMeasurement must not borrow it across a later caller decision.
Before each root, logical lengths/state must reset; an error must not expose
partial state to another root or weaken raw DAG/geometry order. No scratch
entry can become an authority to skip current work.

This is a bounded change around layout.rs/topology.rs and the existing
multi-root caller owner, but its Interface/lifetime migration is more than a
one-line optimization. It retains maximum scratch capacity until the operation
ends, can increase peak memory/liveness, adds borrow/clear plumbing, and may
change generated code adversely. The first root still pays growth. A
single-root operation has essentially no cross-root reuse opportunity; the
near-threshold one-view resize therefore has no corresponding structural
saving from this proposal. Generic scratch/eligible allocation is also
data-dependent, not one allocation per Vec declaration.

The old samples support investigating growth/lifetime, not attributing current
cost to these specific buffers. I would not prepare this candidate before the
current queue and a source-matched diagnostic show remaining material work.
It is one plausible hypothesis, not a demonstrated new path to all-cell twofold.
I found no stronger unqueued low-constant redesign supported by the available
current measurements.

## Decision and stopping evidence

Supported: the retained complete result is 28/42 at the goal; small cells are
2.067–3.867x; exact source/output contracts and successful deletions explain
substantial achieved progress; some apparently cheaper source representations
have qualified but regressed and should remain rejected.

Unsupported: every remaining nanosecond is necessary, the old growth samples
identify current allocator shares, sums of phase medians set an achievable
counterfactual, output richness makes twofold impossible, or another unmeasured
source hypothesis is guaranteed to close the gap.

There is reasonable evidence to stop speculative candidate proliferation while
Root finishes the already reviewed queue. There is **not** qualified independent
evidence to mark the user's twofold goal complete or impossible. A negative
queue result would justify retaining the best measured state and reporting
the remaining gap honestly, not silently redefining success.

If further diagnosis is needed after that queue, the cheapest useful follow-up
is a bounded, source-matched current small-cell call-tree observation of the
existing full probe, with fixture/validation/drop ancestry kept distinct.
Choose one resize and one real edge to distinguish projection and structural
mutation; do not add clocks/counters inside every split or build a profiling
framework. A separate repeat-layout driver must retain its disposal disclaimer.
No such new measurement ran in this lane.

## Evidence bindings

`B` is `/tmp/dockspace-imgui-twofold-20261107-ea4161e`.

| Evidence | SHA-256 |
| --- | --- |
| `B/packed-results-frozen/against-imgui/summary.csv` | `90a3fc3519baa2b74d3e6fe8f373ba7b3e7593afbc144a7a3edd97ddccc37c6b` |
| `B/packed-results-frozen/against-imgui/environment.json` | `7f7f5c24d4eaab9b6189506d39d58d559d079542cc0ed5da5d215484a90cf4f2` |
| `B/packed-results-frozen/source-receipt.json` | `28071ac9e4ed3a71939165c401c3d2d79632d699051b228cd3307e1975a713f8` |
| `B/layout-hotspot/diagnostic-evaluation.json` | `ea06e1e00b297f3c7e3221b6b2f30005f3dfea7e0f713d96658f0321426cc647` |
| `B/layout-hotspot/receipt.json` | `728572a22b83d648a7bbe7bcac1ea2f7141e68a7e9adff2aa2db2f2ab6c63eeb` |
| `B/root-structure-profile/receipt.json` | `d9343a3f35a22fdb267b38c48b245b7fb8f01c07eb5ebab2615d545ee325ee02` |
| `B/small-root-allocations/receipt.json` | `da5467c9b8db790c1e68313234819990d267e9ec3b319cdcb409a5d2cdfceed8` |
| eb94cb2 `crates/dockspace/src/layout.rs` | `ee0bbdadf60b10a3787539e89cdee41411301c3c774e66d53c0146ad1567eaec` |
| eb94cb2 `crates/dockspace/src/layout/topology.rs` | `1dd27eb635c84e0d73bf9186eb2e741f400b10e4b36bb6365477ac01c5bd21c6` |
| eb94cb2 `crates/dockspace/src/operation.rs` | `7586ec06b29b77a794133e0c554d4fcca7a6faa16b8c550b86034ad394175862` |
| eb94cb2 `crates/dockspace/src/docking_cost.rs` | `01d8438fc79095aacae935170aff3ed7eb8b5e497aa368c9966889b789adf2d3` |
