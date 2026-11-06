# Freeze a transaction's report with its completed candidate

PreparedTransaction now belongs to the closed operation owner and privately
stores its completed Workspace and immutable TransactionReport. Callers may
borrow the candidate for preview or consume the paired values. Consumption no
longer takes an arbitrary original Workspace or recomputes the report. All
engine publication barriers and comparisons after later activation/recovery
edits remain in place.

Single policy Select uses its exact selected/MRU result. Single policy Item
Move uses the concrete writer result after successful local canonicalization.
Same-tabs center, equal reorder and exact singleton edge no-ops return before
detachment or issuance. Transfers to another pane change unique membership; a
real Item edge insertion retains a newly allocated nonempty tabs identity.
Canonical repair cannot restore all original NodeIds, even if the resulting
shape, weights and items visually agree. These facts let the owner remove two
Workspace equality calls from that specific Item preparation/consumption path.
The old calls could short-circuit; this is not a claim that two full N-node
walks always ran.

General literal/staged/recovery batches still compare the complete final
normalized candidate once. A net-zero batch can return false while retaining
strict new root issuance after real intermediate edits. A lone general Move
which normalizes to exactly equal values still restores its original issuance.
Failure keeps the original Workspace, and publication moves the same actual
candidate rather than replaying a command sequence.

The existing local/general matrix compares complete values and both reports,
then checks independent placement and strict validity. Its no-op branches now
pass the original source and target through the real checked Move validator,
locking original-reference usability without recapturing. A new same-shape edge
case confirms that a replacement tabs identity makes both reports true. Staged
net-zero/MRU, ignored stage failure, exact-float failure, late rollback, old
references and discard/retry coverage remain.

Counters-enabled checks pass 84 focused and 2,227 workspace tests, formatting,
correctness/suspicious Clippy, strict docs and doctests. The independent public
host harness passes all 292 no-feature and 335 persistence-feature tests.
Independent source review finds no remaining blocker. The full operation still
includes request capture, candidate creation and release of the replaced live
Workspace, all-root fresh layout and fresh complete owned export. Frozen paired
comparisons preserve every complete vector and sampled digest.

This change does not demonstrate a material performance improvement. Against
`6fe5eb6`, 14/42 group/cell medians regress; the full range is -8.11% to +9.09%
improvement. At 1,024 panes the range is -0.93% to +4.37%, with small and mixed
directions. Seventeen p95 and twenty-one max cells regress, with maximum
regressions 75.57% and 864.89%. Retain the private candidate/report ownership
because it closes the mutable pairing Interface and removes repeated report
derivation, not because it proved a major speedup. The original equality calls
usually short-circuit in these specific fixtures; full candidate copying remains
the move-phase target. This is an ownership refactor with measured flat/mixed
cost, and is not counted as a winning performance intervention.

The complete-operation ratios to ImGui are 3.87–8.80x at 16 panes,
2.06–4.30x at 128 and 1.72–2.88x at 1,024. Eight of 42 cells pass 2x, the same
large resize/edge set as before; the goal remains active.

The [archive](2026-11-07-prepared-report/evidence.tar.xz) contains 416 input
files, 16,128 raw rows and 336 complete vectors, source/binary/check receipts,
reconstructible patches and independent review. All members were read back
byte for byte. [Comparisons](2026-11-07-prepared-report/comparisons.csv) retain
all tails and negative cells; [SHA256SUMS](2026-11-07-prepared-report/SHA256SUMS)
identifies the artifacts. No command/clone work-probe rerun is claimed because
their ledger sites are unchanged. The qualified leaf head `5ea7fbb` passes all
six CI jobs in [run 42282205148](https://github.com/Latias94/egui_docking/actions/runs/42282205148);
this refactor and subsequent candidates require their own pushed-head results.
