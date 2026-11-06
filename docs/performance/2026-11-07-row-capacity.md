# Reserve fresh Item structure rows from their previous root size

A checked single-Item move can add at most one tabs leaf and one split wrapper
to a root. Retain only the previous node count while that root is Editing and
use `oldN + 2` as a capacity estimate for its fresh rows. No old RootStructure
Arc survives through this hint. General/birth/raw builders keep zero hints and
their complete placement installer. Hints never limit traversal, assign lengths,
authorize edits or replace validation. A final Tabs root reserves only its
single node directories and allocates no child/split buffers from the old size.

The [independent source review](2026-11-07-row-capacity/review.md) verifies all
state transitions, the checked Item bound and complete ownership/error behavior.
The exact two-file patch is SHA-256
`13a67cd74cddc850dbd8064725ddd90bc7d57efbeb4eb93efe79575aff5d78e6`.
Its costs are real: Editing's usize may enlarge the enum and RootOwner, first
begin reads the old count on non-Item paths too, and a root shrinking to a
smaller Split can retain excess capacity. No actual enum-size, allocator-byte,
peak-memory or exclusive allocator-time result is claimed.

## Qualification and complete comparison

Ordinary counter-enabled qualification passes 428 focused tests, four work
regressions, 2,069 production core, 2,245 workspace, four public integrations and
292/335 host tests. Formatting, correctness/suspicious Clippy, strict docs,
doctests and dependency gates pass. All 21 work rows and 20 columns match the
Item-frontier control exactly, including six successful bodies, 2,926 rows and
all actual payload copy counts. These counters do not record Vec capacities or
allocation bytes. The same three opt-in probes remain skipped.

Frozen executable SHA-256:
`be863f934c761071b9ebc206a0a328941f9d8a12f75e2cb9a8119470e0784a6e`.
Actual control: `3dd308c`, binary
`09a32eeefd9e3781133c6662cf917d97b5277a372e748a339425a6534313dfe9`.
Complete control and pinned ImGui comparisons each use both ABBA/BAAB groups,
48 samples/eight warmups per process/cell and 96 observations per condition/
group/cell. All vectors and digests agree. Request capture, checked mutation,
fresh all-root leaf measurements/projection and complete owned export remain.

Against the control, 27/42 complete p50 group/cells improve and 15 regress,
with no tie. The full reduction range is -10.21% to +11.02%. At 16 panes, edge
operations improve 8.07–11.02% across both view counts/groups. At 128 panes,
edges improve 5.44–9.13%. Large one-view edges improve 3.50–4.43%; large
four-view edges are +2.27% and -0.11%. Retain this bounded reservation change
for the observed edge improvements, without calling it uniform acceleration.

Both 16-pane single-view Center groups regress (-3.16% and -10.21%). Both
large Center view counts regress, as do most large resize observations. These
are complete-operation results, not a proven attribution to the possible larger
owner header. The [full table](2026-11-07-row-capacity/comparisons.csv) retains
every negative: p95 has 17 regressions/two ties and a -11.02% to +17.24%
reduction range; max has 25 regressions and a -407.94% to +85.86% range.

| Total panes | Complete p50 / ImGui, both groups | Group/cells at most 2x |
| --- | --- | --- |
| 16 | 2.53–4.87x | 0 / 14 |
| 128 | 1.51–1.92x | 14 / 14 |
| 1024 | 1.02–1.40x | 14 / 14 |

The total remains 28/42; every small cell still exceeds 2x. Every p50 trails
ImGui, as do 39 p95 and 30 max cells. The goal remains active. Continue the
separate fresh-layout buffer experiments; no source reservation count implies
the remaining target is attained or impossible.

The [archive](2026-11-07-row-capacity/evidence.tar.xz) retains 430 inputs plus
its manifest, 16,128 raw observations, 336 complete vectors, ordinary checks/work,
the exact two source pairs and independent source review. Every member
round-trips against its original bytes.

The [independent evidence review](2026-11-07-row-capacity/evidence-review.md)
binds immutable candidate `e3c5171` to `3dd308c`, verifies every archived
input/vector and recomputes the complete statistics. It finds no evidence or
retention-judgment blocker and confirms the goal remains incomplete.
