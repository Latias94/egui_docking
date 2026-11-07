# Main merge and current-runtime optimization follow-up

The qualified refactor was fast-forwarded into main at `c7508ee`, without
binding any worktree to main. Its [six-job CI run](https://github.com/Latias94/egui_docking/actions/runs/42363449111)
passes, including physical X11 docking, drag-back, release and native window
retirement. The [merge and qualification receipts](2026-11-07-post-main-result/summary.json)
bind that head and the native log. This run qualifies `c7508ee`; a later
publication head still requires its own six-job qualification.

After that merge, nine independent candidates against this retained runtime
were implemented and measured. None earns retention under its predeclared
rules. Every candidate, including its temporary tests, is restored exactly;
all 324 formal source/configuration inputs match the qualified frozen
control. Runtime remains `0e441c8`, executable SHA-256
`3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`.
This continuation retains evidence and analysis, with no new runtime delta.

## Retained result and comparison scope

| Total panes | Complete p50 / ImGui, both groups | Conditions at most 2x |
| --- | --- | --- |
| 16 | 1.8111–3.1244x | 2/14 |
| 128 | 1.2285–1.4969x | 14/14 |
| 1024 | 0.8292–1.1445x | 14/14 |

The retained result remains **30/42**, with twelve small conditions missing;
the numerical target is not achieved.
These are the retained campaign's observations, not a new campaign assembled
from favorable candidate/ImGui ratios. The
[original final report](2026-11-07-twofold-result.md) preserves the full
matrix, original-to-final confirmation, limitations and earlier architecture
changes. The original-to-final complete p50 reductions of 78.82–96.06%
remain a separate direct pairing; incremental candidate effects are not added.

Both products use binary topology in this formal matrix. Sixteen panes is
the total leaf count, not a 16-child split competing against ImGui's binary
tree. Rust's fixture constructs two children per split and its complete
export explicitly asserts binary topology. The general Rust model supports
other arities, but a fanout difference cannot explain this matrix's misses.

The renderer-free timer still includes real request capture, checked docking
mutation, fresh leaf constraints, all-root layout and complete fresh owned
common export. ImGui runs actual pinned docking/root-update/layout code;
neither product renders. Each independent pairing keeps 42 group/cells,
serial ABBA/BAAB, 48 samples/eight warmups per process/cell, complete vectors
and digests, and every p95/maximum observation. All experiments compare
against the actual retained Rust control; separate ImGui counts cannot
substitute for control-relative selection.

## Current-source diagnosis and nine decisions

The [new source-matched diagnostic](2026-11-07-post-main-diagnostic.md)
samples the retained post-merge runtime rather than an earlier architecture.
Its three longer probes preserve 1.5 million diagnostic raw observations.
Independent callsite review removes one inlined fixture subtree from the
resize classification. Static instructions and timed ancestry establish
specific surviving work, without assigning exclusive clock or allocator
shares, causal speedups, or a lower bound.

| Candidate | Initial complete p50 improve / regress / tie | Additional Rust confirmation | Decision |
| --- | --- | --- | --- |
| [Reuse source Tabs nonempty fact](2026-11-07-source-nonempty.md) | 22 / 19 / 1 | 26 / 14 / 2 | Move gates pass, but repeatable material resize losses fail protection |
| [Exact half-share for equal binary weights](2026-11-07-equal-binary.md) | 34 / 8 / 0 | 20 / 20 / 2 | Confirmation fails complete and small-layout gates; original tail concerns retained |
| [Reserve root-build pending stack](2026-11-07-build-pending-reserve.md) | 18 / 22 / 2 | Not warranted | Edge improvements 6/12 and small edge 2/4 fail |
| [Borrow placement child roster](2026-11-07-placement-roster-borrow.md) | 21 / 21 / 0 | Not warranted | Edge improvements 5/12 and small edge 2/4 fail |
| [Insertion-aware shared Tabs copying](2026-11-07-tabs-insertion-cow.md) | 8 / 30 / 4 | Not warranted | Center/cross improvements 2/18 and small 1/6 fail |
| [Remove release prefix bookkeeping](2026-11-07-debug-prefix.md) | 14 / 25 / 3 | Not warranted | Complete improvements 14/42 and small layout 3/14 fail |
| [Borrow Live Tabs target facts](2026-11-07-live-tabs-facts.md) | 10 / 30 / 2 | Not warranted | Center/cross improvements 4/18 and small 1/6 fail |
| [Borrow synchronous topology](2026-11-07-borrowed-topology.md) | 26 / 16 / 0 | Not warranted | Complete improvements 26/42 and small layout 3/14 fail |
| [Scalar root issuance](2026-11-07-scalar-issuance.md) | 18 / 21 / 3 | Not warranted | Move improvements 16/30 and candidate ImGui inventory 29/42 fail |

The first three candidates pass all twelve ordinary gates before measurement.
The last six follow the explicitly declared early-screen protocol: formatting,
targeted tests, focused regressions, ordinary work equality and complete output
comparison first; the other nine gates and confirmation only for a promising
candidate before retention. Their three-gate screening is not represented as
twelve-gate qualification. All nine preserve the complete ordinary work CSV.
The first two confirmations are additive; neither replaces initial negatives.
Scalar issuance also runs its new internal restoration witness separately,
with one test passing; that witness is not an additional ordinary gate.

The private release-prefix experiment explicitly permits removing a defense
against an illegal private row gap while preserving public errors and debug
checks. That scope refinement is recorded rather than silently claiming all
internal panic behavior is unchanged. Its rejection restores the guard too.
The Live Tabs experiment preserves strict verification, complete owner/rule
derivation before mismatches, indexed authority, edge handling, gap error order
and atomic rollback. It removes real atomic retain/drop work but still fails
the complete comparison. These are measured implementation tradeoffs; fewer
source operations are not assumed to imply lower complete latency.

The first continuation challenge after seven decisions identifies a real
remaining temporary topology owner, rather than accepting a stop based on
the earlier failures. Its
[intermediate review](2026-11-07-post-main-result/seven-candidate-continuation-challenge.md)
is preserved alongside the independent
[earlier stop assessment](2026-11-07-post-main-result/seven-candidate-second-stopping-review.md).
The two conclusions differ because of the newly located work. Borrowed
topology is then independently implemented, screened and rejected. A separate
scalar-issuance assessment locates actual root-edit allocation and proposes
keeping selection predecessors in the existing owner, avoiding a history
directory. That implementation also fails its complete screen. Both later
treatments preserve durable output/reference contracts; their failures are
not inferred from the previous seven.

## What remains

The retained implementation still forks an independent generational node
directory while sharing immutable payloads and metadata. Checked commands
retain current-reference authority and preserve independently prepared
candidates, staged mutation and rollback. Fresh layout gathers leaf answers,
composes constraints bottom-up, solves constrained extents top-down and owns
all node/split results. Complete export produces another fresh owned vector.
ImGui updates existing mutable geometry in place. Its actual docking update
also performs normalization and visibility checks; it does not lack validation.

These contract and representation differences are concrete remaining work,
not a measured allocation of the gap. The latest tests reject several specific
duplicate-check and ownership deletions. Earlier persistent-directory,
continuation, axis-check and output-storage alternatives also retain their
negative complete results. Nothing here establishes a Rust or ownership floor.

Compiled projection events would trade the continuation stack for root-build
work, retained traversal/thickness metadata and raw-DAG handling. Explicit
cross-root scratch reuse needs a caller/API migration: the unchanged formal
caller currently projects roots separately, and binary numeric scratch is
already local. Removing fresh input or complete output ownership would change
the declared operation. Temporary split-directory writes remain an unmeasured
local-cleanup hypothesis; deleting them would not remove the required identity
sort, and currently lacks sufficient complete-operation cost evidence.

## Final continuation decision and publication gate

Both updated independent challenges now recommend ending runtime work for
this continuation: [review one](2026-11-07-post-main-result/independent-stopping-review.md)
and [review two](2026-11-07-post-main-result/second-stopping-review.md). The first
reviewer previously identified the additional topology owner and explicitly
withheld its stop recommendation until that screen was resolved. Its current
opinion checks the restored source and the later treatments; the earlier
seven-candidate reviews are preserved as intermediate checkpoints.

The stop rests on resolving concrete located opportunities and assessing
the remaining tradeoffs, not the number of rejected patches. No further
bounded deletion currently has sufficiently supported benefit under this
complete metric. Compiled events, scratch migration, directory changes and
other identity representations remain unmeasured alternatives, not refuted
designs. A new source-bound cost finding or distinct architectural objective
can reopen investigation. Twelve small conditions still miss; no numerical
completion, optimality, impossibility or lower bound is claimed.

The [final decision](2026-11-07-post-main-result/decision.json),
[nine-treatment index](2026-11-07-post-main-result/experiments.json) and
[execution snapshot](2026-11-07-post-main-result/execution-log.json) bind the
source, individual raw archives and review hashes. The nine experiment
archives preserve 161280 raw observations and 3360 complete vectors in total,
including both additive confirmations. The current diagnostic's separate
1.5 million observations do not enter formal selection. Every experiment has
its own independent evidence review and exact restoration record.
The [document review](2026-11-07-post-main-result/document-review.md) finds
no substantive numerical or scope blocker; its suggested witness clarification
is incorporated above.

The reviewed documentation is committed and then fast-forwarded to main.
Publication is complete only when that exact pushed head has all six CI
jobs passing, including the physical X11 coordinator and actual window
retirement. The `c7508ee` run above qualifies the earlier merge, not this
later head. Final push/run/native-log/source receipts are captured after
publication in the execution directory and final handoff rather than changing
the head they qualify. No runtime source changes require requalification
from another candidate: all 324 inputs already equal the fully qualified
retained control, and final remote CI independently exercises that head.
