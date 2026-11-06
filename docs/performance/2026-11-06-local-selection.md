# Local transactions for tab selection

Date: 2026-11-06. Baseline: `42fa235b91e36c4cb471532be4548f82e8015fd0`.
This follows the [selection-cost attribution and duplicate-validator removal](2026-11-06-selection-validation-costs.md).

## Result

Pure single-command selection can preserve the invariants of an already
validated workspace. The engine now carries that qualification with its
immutable workspace owner, so selecting a tab no longer re-establishes unrelated
forest topology and item-ownership invariants through the general rewrite.
The change keeps the complete source fingerprint, policy authorization, existing
selected/MRU edit, equality-based report and all publication barriers.

This is a private implementation change. Public commands, transactions, focus
protocols, frame settlement and rendering workloads are unchanged. It retains
the inexpensive transaction clone measured in the previous investigation.

## Eligibility belongs to the snapshot

`operation::WorkspaceSnapshot` privately owns the workspace `Arc` and its
selection eligibility. It exposes immutable access; requesting a mutable draft
revokes eligibility before `Arc::make_mut` can expose the workspace. Cloning the
owner shares the exact immutable allocation and its qualification. Index
provenance continues to use that same allocation independently.

The initial constructor and workspace replacement use their existing full strict
validation. Strict validation rejects orphan nodes, unprotected empty leaves,
degenerate splits, same-axis nesting, invalid selections/MRU and empty surface
presentations. These cover the general canonicalizer's rewrite and pruning
conditions. Unchanged splits preserve their original weights, so passing strict
validation is also a fixed point for the generic transaction's normalization.

A successful general preparation now produces an opaque `CanonicalWorkspace`.
Only the operation module can construct it. Consuming its typed result into a
snapshot retains qualification. Raw `Workspace` conversion grants none.
Discardable journal follow-ups revoke qualification, run all existing general
checks, and restore it only after success. A failed follow-up leaves its draft
ineligible and the caller discards it.

The specialized preparation matches exactly one `Select` on an eligible owner.
It clones once, runs the same authorization and `select` function, and produces
the typed candidate. That function verifies the complete live root fingerprint,
generational node ID and item membership, then changes only the selected item
and its exact MRU permutation. It cannot change topology, presentation ownership
or item multiplicity. General canonicalization, its full validator and global
item-multiset scans are therefore unnecessary for this preserving edit.

Direct commands, single journal commands, pointer tab presses and timed hover
selection use this preparation interface. Mixed operations, including contained
raise plus selection, still execute the general transaction. Their checked
results can qualify the next pure selection. Legacy close/recovery/preflight
paths that return a raw workspace conservatively lose qualification; the next
selection on such a publication takes the general path once.

This does not make the complete selection transaction constant-time. The full
root source fingerprint, whole-workspace clone, changed comparison, requirement
refresh and scene production still depend on root/workspace size.

## Qualification

The baseline uses the frozen executables built for the previous change. Their
hashes match that archive, and its complete source inventory matches committed
`42fa235`. The candidate differs only in the 13 source/test files recorded in
the new source inventory. Both benchmark sources and pinned ImGui inputs are
byte-identical. The archived provenance identifies the source snapshot rather
than relying on the previous build metadata's pre-commit Git label.

All timing is serial and separate from builds, tests, review and active heap
profiling. Cargo uses one job and existing targets. The product uses the
existing `production` ThinLTO profile; the renderer-free example uses release
ThinLTO and one codegen unit. The pinned reference comparison also runs through
the existing product-comparison script.

The frozen before/after product executables run one ABBA and one BAAB group,
24 samples and six warmups per process, at 16/128/1,024 panes. Each group pools
the actual samples from its two processes per condition, yielding 48 samples
per case. At 1,024 panes:

| Scenario | Group 1 before → after p50 | Change | Group 2 before → after p50 | Change |
| --- | ---: | ---: | ---: | ---: |
| Idle, one frame | 7.946 → 7.916 ms | -0.4% | 8.068 → 8.121 ms | +0.7% |
| Physical tab click, two frames | 29.672 → 28.141 ms | -5.2% | 30.288 → 28.649 ms | -5.4% |
| Surface resize, one frame | 18.674 → 18.667 ms | -0.0% | 18.992 → 18.756 ms | -1.2% |

The complete mesh, content, frame and pass counts match before/after for every
sample. Idle and resize do not establish a direct benefit from this selection
change. The second group's candidate click maximum is 37.953 ms versus the
baseline's 31.101 ms; its p50 and p95 still improve. The archive retains this
outlier and every scale's p95/max; no general tail-latency improvement is claimed.

The unchanged renderer-free `frame_cost` workload also runs ABBA and BAAB,
48 samples and six warmups per process. It measures, compiles, records all ordered
paint operations, accepts the receipt, commits the frame and settles output.
It alternates two items through a semantic action, rather than emulating the
product benchmark's two-frame pointer click. At 1,024 panes, the four process
p50 values for each condition are:

| Update scenario | Four baseline process p50 values | Four candidate process p50 values |
| --- | --- | --- |
| idle | 1.884, 1.938, 1.928, 1.902 ms | 1.886, 2.092, 1.912, 1.908 ms |
| select | 13.244, 13.408, 13.380, 13.455 ms | 11.969, 13.030, 11.995, 11.994 ms |
| measurement_change | 12.651, 12.619, 12.748, 12.735 ms | 14.116, 12.977, 12.641, 12.898 ms |
| drag | 13.993, 14.378, 14.171, 14.210 ms | 14.155, 15.193, 14.213, 14.279 ms |
| outputs_in_flight | 13.417, 13.362, 13.336, 13.366 ms | 12.597, 12.128, 12.085, 12.036 ms |

These are process summaries, not pooled sample percentiles. Individual timings
are unavailable in this example and are not reconstructed. All process p95/max,
settlement timings, drag and outputs-in-flight cases remain in the archive.
`measurement_change` changes both bounds and tab intrinsic measurements. Its
process timing variation is retained; no change to that path is attributed to
this refactor.

Separate active-DHAT runs use six samples and two warmups. Selection update
allocation accounting is:

| Metric | Baseline | Candidate | Reduction |
| --- | ---: | ---: | ---: |
| Allocation events | 103,852 | 84,659 | 19,193 (18.5%) |
| Requested bytes | 53,610,952 | 51,861,180 | 1,749,772 (3.3%) |

Selection settlement remains at 1,832 allocation events and 587,296 requested
bytes. Idle, drag and measurement-change allocation counts and requested bytes
are identical before/after at all three scales. Requested bytes are aggregate
allocation requests, not resident memory or RSS.
The complete allocation output also retains live-before/live-after and prefix
peak accounting. Allocation-profile timing is not used to qualify latency.

## Correctness and remaining work

Seven new focused regressions cover differential selection/MRU sequences under
locked and unlocked policy, reselection, stale/wrong/missing sources, actual
node-slot recycling, unknown and corrupted snapshots, general mixed-batch
atomicity, qualification after checked follow-ups, direct versus product focus
semantics, and failure of a later input after a selection inside an uncommitted
frame. The generated test randomizes selection sequences and policy; its forest
fixture includes multiple leaves, an empty protected central leaf and contained
roots, rather than arbitrary generated topology.

All 2,158 workspace tests, 292 default-host tests, 335 serde-host tests, 556
native tests and 10 doctests pass on the final source snapshot. Normal suites
skip the opt-in layout probe and the existing native wall-time case. Workspace
and native correctness/suspicious Clippy, formatting, dependency boundaries and
whitespace checks pass. Independent static review found no blocking issue;
its qualification-propagation finding was fixed and independently rechecked.

The previous attribution still points to requirement/snapshot regeneration,
scene compilation and scene validation/linking as larger remaining costs. The
next useful refactor is to retain structure-dependent computation across
selection changes and construct shared scene inventories once. Command
fingerprints must keep selection/MRU freshness semantics. Selected content can
also change policy or measured minima, so unchanged topology alone does not
authorize geometry reuse.

The [evidence archive](2026-11-06-local-selection/README.md) includes exact
source/binary hashes, raw product samples, every headless process summary,
allocation results, gate logs, reviews and a standalone verification script.
