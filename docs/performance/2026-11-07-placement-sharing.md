# Share exact placement metadata until an actual write

Wrap the existing generational placement directory in one Arc. Workspace
candidates share it until an actual insertion, parent/root/index change or
retirement. Complete RootStructure installers retain their original full writes.
`place_node` compares the complete requested placement before COW, while
`forget_placement` checks the raw exact key even after arena retirement.
Root reference issuance, policy, canonical repair and output ownership remain.

This is whole-directory COW. A structural edit still clones the entire table on
its first shared write; this slice does not make candidate creation O(frontier).
It adds an Arc allocation for an empty owner, handle operations, indirection and
pre-write probes. The existing discardable follow-up's `mem::take` constructs a
default owner and can pay the new empty allocation.

The [independent source review](2026-11-07-placement-sharing/review.md)
checks all five write entrances, complete generational identities and discarded
candidate isolation. Its original new fixture omitted the policy permission for
Native rehome. That first focused attempt passed 427 tests and failed that one
fixture with `PresentationModeDisabled { mode: Native }`. Enabling the required
test policy changes no runtime byte or assertion. The initial failure, exact
test-only patch and
[supplemental review](2026-11-07-placement-sharing/test-policy-review.md)
are retained.

## Qualification

With ordinary counters enabled, corrected formatting and all 428 focused tests
pass, followed by four work regressions, 2,069 production core tests, 2,245
workspace tests, four no-default public integrations and 292/335 public host
tests. Correctness/suspicious Clippy, strict documentation, doctests and dependency
checks pass. The three opt-in probes remain skipped in the ordinary suites.
The final qualification receipt explicitly combines the two successful serial
segments with identical source hashes; the initial failed receipt is preserved.

The ordinary work probe retains all 21 cells and 20 columns. RootStructure builds
remain zero for resize and center/cross moves and one per edge cell, with the
same rows/split handles. Actual MRU element copies remain 0/4/2 for
resize/center-or-cross/edge. There are no fingerprint or item-multiset rescans.
Those counters do not measure placement allocator events or bytes.

## Complete paired results and decision

The frozen executable is SHA-256
`edefa3e49f01b79a395b29125ca2b87acb467c50ce387e5b0856686dab88ae01`.
The actual control is MRU commit `8654090`, binary
`cba8437ec30e46186ec8c644b0474e8a97d4531ab34d0d727deab8f59728f31e`.
Both the control and pinned ImGui pairings independently use the unchanged
ABBA/BAAB groups, 48 samples/eight warmups per process/cell and 96 observations
per condition/group/cell. All complete output vectors and digests agree.
Request capture, checked mutation, fresh leaf answers, all-root projection and
fresh complete owned export remain timed.

Against the control, 17/42 complete p50 group/cells improve, 24 regress and one
ties; the full reduction range is -7.78% to +6.46%. At 1024 panes, center and
cross-view moves improve 2.16–6.46% in both groups. Their mutation medians fall
from roughly 11.0–11.2 microseconds to 6.1–6.2 microseconds, consistent with the
removed eager placement copy. These are independently recorded phase medians,
not additive savings or an exclusive allocation-time attribution.

Small-root results remain mixed or worse. Both 16-pane one-view edge groups
regress, as do four-view cross moves and most small resize cells. Both large
one-view edge groups also regress. The full
[comparison table](2026-11-07-placement-sharing/comparisons.csv) retains every
negative, including p95's 21 regressions (-23.77% to +18.80% reduction) and
max's 25 regressions (-757.09% to +90.23%). Keep the sharing mechanism for its
observed large nonstructural mutation reduction and old-candidate ownership;
it is not a uniform whole-operation performance winner.

| Total panes | Complete p50 / ImGui, both groups | Group/cells at most 2x |
| --- | --- | --- |
| 16 | 2.53–4.80x | 0 / 14 |
| 128 | 1.52–2.12x | 12 / 14 |
| 1024 | 1.08–1.44x | 14 / 14 |

This campaign has 26/42 group/cells within 2x. Every p50/p95 remains slower than
ImGui; 36 max cells are slower. The goal remains active. Next test the separate
Item completion slice that removes placement reinstallation only after the
checked writer has already repaired every actual join, then fresh layout buffer
ownership. Do not infer that a one-cell threshold change proves a uniform gain.

The [evidence archive](2026-11-07-placement-sharing/evidence.tar.xz) contains
436 inputs plus its manifest: 16,128 raw observations, 336 complete vectors,
qualification/work receipts, both actual source versions and the policy-fixture
correction. Every archived input round-trips against its original bytes.

The [independent evidence review](2026-11-07-placement-sharing/evidence-review.md)
binds immutable candidate `c98569d` and control `8654090`, reproduces all raw
aggregates/vectors and published comparisons, and finds no blocking evidence
issue or unsupported uniform-win claim.
