# Complete cold mutations from their actual effects

The checked command and canonicalization owners now complete their own actual
graph effects. Remove `ColdRootTransition`, whole-root before/after fingerprints,
the cold command classifier and the repeated initial per-root carrier search.
This follows the shared-payload checkpoint `fbc8659`; it does not change the
primary docking measurement or replace checked mutation with a second executor.

Canonical rewriting returns changed roots and exact retired generational NodeIds
from its existing traversal and reachability sweep. An unchanged split retains
its existing payload and f32 bits. Removing a child, collapsing a container or
flattening a same-axis split records an actual rewrite. Completion forgets the
retired placement rows and rebuilds placement only for roots actually rewritten.
The input structural gate, normalization formulas, before/after item conservation
and complete strict output gate remain in their existing order.

Concrete Open, Move, root birth, donor detach, rehome, promotion and approved-close
writers complete their actual effects before the next staged capture/check.
Rectangles and peer roster order retain their separate preconditions. A literal
no-op retains issuance; a general single Move normalized to exact original values
still restores its original reference. A multi-command A→B→A transition retains
its intervening fresh issuance even when the final durable report is false.
The candidate is still isolated, and a late failure cannot publish partial work.

Initial Builder/import issuance discovers actual main/contained carriers once,
stably sorts by RootId and invokes the same concrete birth completion. Full raw
canonicalization and strict validation still precede issuance. Stable first-match
ordering preserves the former private malformed-input carrier preference, and
foreign/missing carriers cannot substitute for an absent actual root. The deleted
`refresh_root_placement` has no remaining consumer; `presentation_in_storage`
retains its real geometry, diagnostic and fallback consumers.

Two registered cold-effect cases cover prefix ABA and a literal normalized inner
edge no-op followed by a frozen command. The latter does not claim to exercise a
real detach/rewrite returning equal values. Two initial-issuance cases cover mixed
carriers, fresh independent-build references, exact placement, empty storage and
typed presentation rejection. Existing staged rollback, full local/general
oracles, exact float frontiers, recovery and public old-output tests remain.

New root birth still walks its actual members. Canonical completion may traverse
an entire rewritten root, its two effect Vecs allocate when populated, and general
batch reconciliation/equality still run. Initial discovery retains BTreeMap
lookups and O(R log R) stable sorting, with possible temporary storage. Builder
fixture construction is outside the primary docking timer, so any initial-issuance
improvement must not be presented as a primary-kernel speedup.

## Executed qualification and complete measurements

Counters-enabled qualification passes 340 focused cases, four work regressions,
2,062 production-profile core tests, 2,238 workspace tests and four no-default
public integrations. The three opt-in probes remain skipped in ordinary suites.
Both independent public host configurations pass, with 292 default and 335
persistence-feature tests. Formatting, correctness/suspicious Clippy, strict docs,
doctests and the dependency boundary check pass. All ordinary source hashes match
the frozen timing inputs. Independent source reviews cover both exact patches.

The existing production recovery-scale test reports about 0.008 seconds, compared
with about 0.042 seconds at the copy checkpoint and 0.026 seconds at `91f42ce`.
It covers actual 16/128/1,024-root recovery and now asserts zero root-fingerprint
builds/visits. This is a coarse nextest duration including fixture, assertions and
ordinary counters, not a formally isolated or paired cold-operation benchmark.
The primary docking fixture does not exercise those cold recovery transitions.

Retain the ownership cleanup and removal of repeated cold scans, accepting its
observed primary-kernel costs. It is **not a demonstrated hot-path speedup**.
Against frozen `fbc8659`, 26/42 complete p50 group/cells regress; the full change
range is -12.51% to +7.78% improvement. At 1,024 panes the range is -1.96% to
+3.95%, with mixed directions between the two groups. Twenty-seven p95 and
twenty-four maximum observations regress, with worst regressions 164.69% and
603.51%. No attribution to noise, code alignment or another unmeasured cause is
claimed for these negative observations.

| Total panes | Complete p50 / pinned ImGui | Group/cells within 2x |
| --- | ---: | ---: |
| 16 | 4.16–7.80x | 0/14 |
| 128 | 1.84–3.35x | 2/14 |
| 1,024 | 1.33–2.14x | 8/14 |

This campaign has 10/42 group/cells within twice ImGui, versus the copy campaign's
13/42. These are separately paired campaigns, and neither removes the other's
results. Every median remains slower than ImGui; forty p95 and thirty-four maxima
are also slower. The target remains unmet. Fresh layout and small-root fixed costs
remain actual candidates for further structural work.

Both campaigns keep the original 21-cell matrix, 48 samples/eight warmups per
process, serial ABBA/BAAB groups, full timed request/checked mutation/all-root
fresh layout/complete owned export, and ImGui pin
`d88f238d1d363ef387382b2e29ad12cf96f826ac`. All complete vectors and sampled
digests agree. Neither frozen executable changes during measurement. Timing
disables only the test work ledger; ordinary qualification keeps it enabled.

[The archive](2026-11-07-cold-effects/evidence.tar.xz) retains 451 input files,
16,128 raw rows, 336 complete vectors, exact build/check/run receipts, complete
ordinary logs, the actual copy baseline receipt, both proposals and their source
reviews. Every member was read back and compared with its original bytes.
[Comparisons](2026-11-07-cold-effects/comparisons.csv) retain every p50, p95,
maximum and negative cell. No earlier generic baseline metadata substitutes for
the actual `fbc8659` control.
