# Reject the borrowed placement roster after the performance screen

Borrowing an unchanged child roster removes a real temporary Vec copy but
fails the predeclared affected-workload latency thresholds. The one modified
authority function is restored byte for byte to actual control `c7508ee`.
Runtime `0e441c8` and the retained 30/42 formal target result remain unchanged.

The current-source sample and disassembly locate allocation, memcpy and free
inside `refresh_child_placements` under timed edge preparation. The candidate
borrows immutable node storage while mutating the disjoint placement map,
preserving the parent's first placement query, non-Split return, complete
generational child existence, complete `NodePlacement` equality and lazy
`Arc::make_mut`. Other `place_node` callers remain unchanged. It removes
temporary roster ownership, not necessary placement-map COW or actual Node
payload copying. The [source design](2026-11-07-placement-roster-borrow/source-review.md)
and [independent contract review](2026-11-07-placement-roster-borrow/contract-review.md)
find no semantic blocker.

## Explicit early-screen qualification

Only three normal-counter ordinary gates run: format, the 432-test targeted
contract set and four focused regressions. They pass, including the existing
placement sharing/discard, generation, local Item/MRU and parent-index
witnesses. The complete 21-by-20 work CSV matches the actual control, and
complete performance vectors/digests agree. The predeclared protocol defers
the other nine ordinary gates until a promising performance result. This
candidate never reaches that stage; twelve-gate qualification is not claimed.

Control binary:
`3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`.
Candidate:
`7a459de3b9653943a2fd1693ecb1805bb481cdacd6870b67de228e20065462d0`.
Both complete comparisons preserve the original 42 group/cells, renderer-free
timer and fresh complete owned outputs. Serial ABBA/BAAB, 48 samples/eight
warmups per process/cell yield 96 observations per condition/group/cell.

| Selection/result | Observed | Predeclared requirement |
| --- | --- | --- |
| Edge complete p50 improvements | 5/12 | At least 8/12 |
| Small-edge complete p50 improvements | 2/4 | At least 3/4 |
| Candidate/ImGui at most 2x | 30/42 | At least 30/42 and all medium/large |

Both affected thresholds fail. Whole p50 has 21 improvements and 21
regressions, reductions -8.90% to +9.12%. P95 has 17/23/2, -41.21% to
+51.88%; maximum has 15/27/0, -574.02% to +91.06%. The independent ImGui
campaign does not override the actual Rust-control result. All negative
and favorable cells remain in the [comparisons](2026-11-07-placement-roster-borrow/comparisons.csv).
There is no additional confirmation or remaining correctness build for an
already rejected performance screen.

The [archive](2026-11-07-placement-roster-borrow/evidence.tar.xz) preserves
419 inputs, 16128 raw observations and 336 complete vectors, exact source
pairs, the three gates, work/build receipts, scripts and source reviews.
The full manifest round trip verifies its bytes. The
[independent evidence audit](2026-11-07-placement-roster-borrow/independent-evidence-review.md)
reproduces the counts, statistics and qualification boundary and confirms
exact authority-source restoration. The [decision](2026-11-07-placement-roster-borrow/decision.json)
records rejection. Removing an optimized-binary allocation is a concrete
work deletion; this result does not establish a faster complete operation
or a universal rule against borrowing disjoint fields.
