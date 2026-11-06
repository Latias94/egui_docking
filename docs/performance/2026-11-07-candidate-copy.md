# Share immutable Node and MRU payloads across transaction candidates

This candidate retains the stock generational SlotMap allocator and actual
checked executor while sharing immutable Node/MRU payloads. NodeStore privately
owns `SlotMap<NodeId, Arc<Node>>`; TabMruStore privately owns the current MRU
map with shared Vec values. Read methods still return ordinary references.
Their only partial-write entry calls `Arc::make_mut`, so modifying a candidate
cannot mutate another Workspace, Builder, prepared candidate or retained snapshot.
No generic provider, persistent validity flag or second executor is introduced.

Every candidate independently clones the allocator, maps and normative placement
directory. Slot generations, free-list state and complete NodeIds remain exact.
Only payloads are shared. Root issuance, source/target checks, selected-first
MRU, policy, numeric gates, final canonicalization, strict validation and atomic
rollback retain their existing owners. Public apply still releases the replaced
Workspace inside its original operation interval; destruction is not deferred
to manufacture a win.

Whole-record replacement swaps a handle rather than first copying the old
payload. Removal transfers a handle, and immutable retain drops unreachable
handles without detaching them. Builder replacement still returns its required
owned previous Node using `Arc::unwrap_or_clone`; that genuine copy remains
counted. Children remain flat NodeIds, so sharing adds no recursive ownership
or drop chain.

The existing logical clone ledger continues to count a complete N-node
candidate inventory. Additional work columns separately record store forks,
live handle copies and actual Node/MRU Clone events with their copied vector
lengths. Those counts exclude allocator extent/holes, capacity, fresh insertion
vectors, ordinary domain snapshots and authority/placement copies. A shallow
fork remains O(arena extent plus live handles/maps), and is never reported as
O(1) or zero copying.

The proposal adds an Arc allocation/header per Node and MRU, atomic handle
clone/drop work and pointer indirection during full graph traversal. Builder
and import therefore have a real resource cost, even though fixture creation
is outside this particular kernel timer. Complete operation measurements must
decide whether the reduced Vec copying outweighs those costs. Peak memory and
allocation bytes cannot be inferred from the payload counters alone.

Seven registered tests cover actual sharing, two-pane mutation and unique
follow-up, raw Builder isolation, no-copy sweep/removal, owned replacement,
late second-command typed failure with full rollback, and independent generation
reuse. Existing local/general report/MRU matrices, exact owner/placement/ABA,
float witnesses, full layout oracle, 10,000-depth and old-output contracts remain.
Counters-enabled qualification passes 124 focused, 2,058 production core,
2,234 workspace and four no-default-feature public integration tests, formatting,
correctness/suspicious Clippy, strict docs, doctests and dependency boundaries.
The three opt-in probes are excluded from ordinary suite counts. Independent
source review covers the exact implementation and two test/counter additions.
The independent public host suites also pass 292 no-feature and 335
persistence-feature tests. The ordinary 21-cell work probe confirms zero root
fingerprint work/item scans and one full logical candidate fork per move.

At 1,024 panes the fork copies 2,047/2,044 live Node handles for one/four views
and 1,024 MRU handles. Center and cross-view transfers actually clone two Node
payloads/four tab items and two MRU payloads/four history items. Edge clones two
Node payloads/two tab items/two child IDs/two weights, and one MRU/two history
items. Resize clones none of those stores or payloads. These are actual Clone
events within the original sample boundary, not estimates from strong counts;
they retain the counter exclusions stated above.

The production recovery scale test takes about 0.042 seconds, versus the earlier
0.026-second `91f42ce` observation. That coarse nextest duration is not a formal
cold-path speed comparison, but the negative remains and effect snapshots are
not claimed solved.

## Complete operation measurements

Retain the candidate. Against the frozen prepared-report `93f2c57` executable,
1,024-pane center, edge and cross-view operations improve 25.39–29.55% in both
serial groups. Resize regresses 1.55–5.47% at that size. The changed storage adds
an indirect Node read and shared-handle overhead even when no transaction forks;
the complete measurements include that cost. Across all 42 group/cells, ten
medians regress; changes range from a 7.91% regression to a 30.77% improvement.
Eleven p95 and fifteen maximum observations regress, by as much as 123.92% and
979.03%, respectively. All raw observations and tails remain in the archive.

| Total panes | Complete p50 / pinned ImGui | Group/cells within 2x |
| --- | ---: | ---: |
| 16 | 3.70–7.20x | 0/14 |
| 128 | 1.74–3.19x | 2/14 |
| 1,024 | 1.29–2.04x | 11/14 |

The total is 13/42 within twice ImGui, versus 8/42 at the preceding checkpoint.
All medians remain slower than ImGui itself. Thirty-five p95 and twenty-seven
maximum observations are also slower. Individual tail reversals establish no
general tail advantage. This is a useful move improvement, not completion of
the twofold target.

At 1,024 panes, independently observed move mutation medians fall to about
18–19 microseconds from the preceding approximately 78–81 microseconds. Fresh
all-root layout remains about 110–113 microseconds, and complete owned export
about 15–16 microseconds. Phase medians are separate distributions and must not
be summed or subtracted to infer an exact per-sample budget. The remaining
layout work and the small-root fixed costs justify the next structure/output
ownership experiments; payload sharing alone does not solve them.

Both comparisons retain 48 samples and eight warmups per cell, 16/128/1,024
total panes, one/four logical views, the original applicable operation roster,
serial ABBA/BAAB groups and unchanged request/mutation/layout/export scopes.
The ImGui pin remains `d88f238d1d363ef387382b2e29ad12cf96f826ac`.
All complete state vectors and digests agree. The counters-enabled qualification
source hashes match the frozen timing source hashes; only the test work ledger
is disabled for timing. Neither executable changes during comparison.

[The evidence archive](2026-11-07-candidate-copy/evidence.tar.xz) contains 16,128
raw observations, 336 complete state vectors, exact commands, source/binary
receipts, the original candidate and source review, ordinary logs and the real
21-cell work CSV. Every regular archive member was read back and compared with
its input bytes. [Comparisons](2026-11-07-candidate-copy/comparisons.csv) retain
all medians, p95 values, maxima and negative cells; the work CSV remains separate
from the unchanged formal timing schema.
