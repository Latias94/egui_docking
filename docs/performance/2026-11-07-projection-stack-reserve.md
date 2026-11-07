# Bounded initial reservation for the projection continuation stack

Retain one initialization change against immutable `49baba1`: use standard
`Vec::with_capacity(topology.nodes.len().min(8))`, then push the same first
`ProjectionFrame::Enter`. Traversal, arithmetic, checks, split results, all fresh
leaf answers and independently owned outputs remain unchanged. Deep roots keep
normal Vec growth; there is no new dependency, public API or inline container.

The [current-source diagnostic](2026-11-07-small-projection-diagnostic.md)
identified this exact Enter-frame growth under timed layout, excluding fixture
ancestry. Its sample counts were never used as a performance fraction or a
numerical forecast. The independent source review binds the candidate to the
retained gather/exact-total parent. A single bounded capacity was selected from
the observed small-tree depth; there was no capacity sweep.

This retains an initial heap allocation for every root. On this build the
48-byte frame means a seven-node root reserves 336 bytes rather than the old
192-byte peak; the extra 144 bytes are temporary, one root at a time. Other
shapes and allocation classes can cost more. It is not a universal reduction
of capacity, allocations, memory or latency.

## Complete measured result

All twelve ordinary gates pass with normal counters, including 2073 production
core tests, 2249 workspace tests, 432 targeted cases, 292/335 host cases and the
existing deep/oracle/raw/error/lifetime regressions. No implementation-mirroring
test was added for a capacity hint. The entire 21-by-20 work ledger is byte
identical to the actual control. The original complete operation is timed in
two serial ABBA/BAAB groups, 48 samples/eight warmups per process/cell and 96
observations per condition/group/cell. All complete vectors and digests match.

Candidate SHA-256:
`3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`.
The control remains the qualified gather binary
`6d255458eac25cd9dcf629b88cfca6959c78db7ed2bb7ccf08fe2cba6332dcfd`.
Complete p50 improves 28 cells, regresses 12 and ties two; the range is -3.93%
to +9.08%. P95 improves 23, regresses 18 and ties one, with a -79.10% to +61.15%
range. Maximum improves 25 and regresses 17, with a -444.03% to +86.36% range.
All negatives remain in the [complete table](2026-11-07-projection-stack-reserve/comparisons.csv).

The predeclared rule required at least 28 complete median improvements, all
128/1024-pane conditions still within 2x, threshold inventory at least 30/42,
and no layout p50 regression in either group of the two preselected diagnostic
cases. These criteria are met. Four-view small resize complete medians improve
8.57% and 8.76%. Single-view small edge layout improves in both groups, while
its complete operation ties in group 1 and improves 1.28% in group 2. This is a
bounded fixed-cost gain, not proof that every median or tail improves.

| Total panes | Complete p50 candidate / ImGui, both groups | At most 2x |
| --- | --- | --- |
| 16 | 1.8111–3.1244x | 2/14 |
| 128 | 1.2285–1.4969x | 14/14 |
| 1024 | 0.8292–1.1445x | 14/14 |

The two passing small conditions remain single-view resize. Twelve small
conditions miss. Against ImGui, 38 p50, 35 p95 and 27 maximum cells trail.
The target is still 30/42. Different campaigns have different observed control
latencies; do not combine this campaign's ratios with another's absolute times.

The [archive](2026-11-07-projection-stack-reserve/evidence.tar.xz) roundtrips
425 inputs, 16128 raw observations, 336 complete vectors, all gates, the source
review and the decision rule recorded before measurement. The
[decision](2026-11-07-projection-stack-reserve/decision.json) retains exactly this
single initialization delta. The
[independent full evidence audit](2026-11-07-projection-stack-reserve/independent-evidence-review.md)
finds no blocking source or evidence issue; its SHA-256 is
`44c6f892b4d831bd46eb3283a7cd9b0b8aa5e92e5e78084175d8e85d9c53d3ab`.
