# Arena bookkeeping control before local transaction redesign

This is an intermediate, conservative comparison in the
[twofold docking plan](../plans/2026-11-07-001-docking-twofold.md). It does not meet
the final ImGui target or conclude the optimization effort.

The runtime starts from `ea4161e`. The strict validator combines node visit state
and root owner into one generation-checked `SecondaryMap`. Canonicalization uses
the same existing arena side-table type for parent, in-degree, rewrite-result and
whole-workspace reachability bookkeeping. Rewrite results share one table across
roots, rather than allocating an arena-sized table for each root. Live-node
lookups precede side-table insertion. Stable ItemId/root/presentation maps and
small root-local diagnostic sets keep their original representations.

Every prior check, full candidate clone, item scan, graph normalization and strict
postcondition remains. This changes node association storage rather than claiming
that validation is unnecessary. Slot generations remain part of lookup; no
hash-only proof, unchecked write, validity flag or persistent proof cache is added.
Side-table storage scales with the backing arena's slot extent, which may exceed
live node count after deletions. It is workspace-local and shared across the
global pass, not replicated per small root. The layout redesign separately keeps
its outputs proportional to reachable nodes.

## Complete-operation measurements

The unchanged renderer-free protocol includes fresh request capture, checked
mutation, every root's full projection and complete fresh owned export. Setup,
validation, digest and retained-output disposal remain outside timing. Every
sample passes the full expected-state oracle; every sample-zero vector agrees.
The production timing executable compiles out only test work counters.

Two serial ABBA/BAAB groups use 48 samples/eight warmups per process over the
complete 16/128/1,024 total-pane by one/four-view matrix. The following percentages
are reductions of full-operation p50 relative to the frozen `ea4161e` executable:

| 1,024-pane case | Views | Group 1 | Group 2 |
| --- | ---: | ---: | ---: |
| Center move | 1 | 22.63% | 22.99% |
| Edge split | 1 | 19.80% | 20.63% |
| Center move | 4 | 27.63% | 25.40% |
| Edge split | 4 | 24.92% | 26.28% |
| Cross-view move | 4 | 25.38% | 24.80% |

All move cells improve in both groups at every size. Resize has mixed results:
1,024-pane one-view changes from 518.209 to 521.750 us in group 1 and from
531.250 to 510.875 us in group 2. At 128 panes/four views, resize p50 is slower
in both groups by 2.09%/5.97%. Resize does not execute these changed runtime
functions during timing; changed fixture preparation and process conditions are
not isolated by this comparison. Retain the negatives, rather than claim a
universal improvement or assign an unproved cause.

Tails also remain mixed. One-view 1,024-pane resize maximum in group 1 rises
756.000 to 1,557.750 us; four-view 16-pane resize maximum in group 2 rises
7.958 to 40.084 us. A 16-pane one-view center-move maximum rises 36.875 to
104.916 us in group 2. Raw p95/max values remain in the archive.

A separate unchanged ImGui-kernel comparison retains another two full groups.
Pooled 1,024-pane center ratios are still about 20.81x at one view and 17.38x at
four views; resize remains 7.44x/6.35x. These are not acceptable final twofold
results. The control supports a structural cost in generic node hashing and
bookkeeping, while motivating removal of redundant global settlement and
per-split layout allocation next.

## Attribution and verification

A baseline CPU sample restricted to subtrees under actual transaction preparation
excludes fixture construction and outside-timer validation. Of 1,146 sampled
preparation observations, canonical completion accounts for 545, canonical input
checks for 236, live root-proof construction for 124, outer item inventories for
101 and workspace cloning for 50. Canonical children are grouped under their
owner rather than counted again. These are sampled proportions, not exact clock
phase budgets, allocation counts or guaranteed savings.

The ordinary all-feature production nextest run passes all 2,024 core tests,
with three opt-in probes skipped. It uses enabled work counters. Existing
canonical/validation diagnostic-order, malformed graph, rollback, recovery and
same-version stale-reference tests remain intact. The exact two-file source
patch, build/source/binary receipt and both 8,064-row comparisons are frozen
before the broader Workspace/layout implementation begins.

See the [evidence bundle](2026-11-07-arena-bookkeeping/) for raw observations,
complete sample-zero vectors, commands and independent review. Later source
changes in the shared worktree are separately identified; frozen executable
hashes bind this control to its own source receipt.
