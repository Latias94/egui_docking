# Reject persistent node values with candidate-local allocation

Reject this exact persistent NodeStore and all accompanying dependency/lock
changes. It keeps the SlotMap allocator behind Arc with unit values and stores
canonical Arc<Node> values in upstream rpds HashTrieMapSync. All thirteen
private interfaces, complete generational keys, slot-order iteration, free-list
order, return ownership and checked candidate authority remain. No custom
persistent container, unsafe key decoding, alternative backend or new proof
layer is introduced. The original author parent is a2365aa; its six edited
files and five lock baselines equal actual retained 37e388f before replay.

All five locks were actually resolved by Cargo. Only rpds 1.2.1, archery 1.2.3
and triomphe 0.1.17 are added, plus the dockspace dependency edges. Every other
existing package stanza/version/checksum is unchanged. Historical evidence
locks remain untouched. An initial dependency-selected tree command rejects
workspace-only --all-features; its result and corrected resolution are both
preserved. No hand-written transitive closure is used.

All twelve ordinary gates pass, including the two new real candidate-branch/
late-failure and sparse/dense allocator-churn regressions, existing independent
layout/numeric oracles, MRU, ownership and 292/335 host suites. Work retains
21 rows/20 columns: nineteen columns equal actual control, while the old N-handle
fork field becomes exactly two explicit root-owner clones per fork. This does
not count rpds internal path/Entry/payload handles or allocator COW. Six root
bodies and2,926 rows remain. Both serial ABBA/BAAB campaigns preserve all vectors
and digests with 48 samples/eight warmups per process/cell and 96 observations per
condition/group/cell. Original timed request, checked mutation, all-root fresh
layout and complete fresh owned export remain unchanged.

Candidate SHA256: `0a48c0edeaad9185ab85efd0e2671247d72026c24a7aed37e31bd8b0a7758b45`.
Actual control is 37e388f, exact-total binary
`ccbc1be2fe9d0c14b63e8fbed845fda6332148786485c539b9166d6efdcca5f3`.
All 42 complete medians regress 67.75–151.02%. P95 regresses 41/42, range
-245.53% to +12.65%; max regresses 35/42, range -537.67% to +76.99%. Resize
also regresses without candidate-fork savings. The
[complete table](2026-11-07-persistent-store/comparisons.csv) preserves every
adverse and favorable tail. These observations reject this read/update/
ownership tradeoff; they do not isolate hashing, path allocation or reference
count shares, or imply that all persistent stores are unsuitable.

| Total panes | Complete p50 candidate / ImGui, both groups | At most 2x |
| --- | --- | --- |
| 16 | 3.8672–6.8000x | 0/14 |
| 128 | 2.5100–3.1786x | 0/14 |
| 1024 | 2.0268–2.7862x | 0/14 |

All 42 p50 and p95 cells and 37 max cells trail ImGui. None meets 2x; the actual
retained exact-total version remains 28/42. Source-level elimination of the
N-handle clone is not a complete-operation improvement.

The [archive](2026-11-07-persistent-store/evidence.tar.xz) preserves 544 inputs
plus manifest, 16,128 raw observations, 336 complete vectors, ordinary/work gates,
actual resolution attempts/closure, before/resolved/formatted locks and source,
original source package/review and exact measured-control pairs. The
[decision](2026-11-07-persistent-store/decision.json) binds the frozen patch and
verifies complete 11-path restoration to 37e388f, including all five build locks.
The retained native motion-receipt fix is outside this core treatment and is
not reversed. Continue binary measurement gathering on the retained control.

The [independent evidence review](2026-11-07-persistent-store/evidence-review.md)
reproduces all recorded results, real dependency resolution and exact eleven-
path restoration. It verifies the 19-column/explicit-owner work boundary and
that the separately retained native fix is unchanged.
