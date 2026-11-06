# Exact root ownership and linear runtime layout

This checkpoint follows `91f42ce`. It replaces command-time root value proofs
with opaque exact references issued by the committed graph owner and compiles
sealed runtime layout trees without a second visited/child-lookup pass. It keeps
the actual checked executor, canonicalization rules and complete owned output.
This is an intermediate optimization checkpoint; it does not establish a
twofold result or an irreducible performance limit.

## Contract and implementation

An unchanged Workspace clone shares its root issuances. An independent Builder
build or an actual root edit issues a different reference, including an edit
which later restores equal durable values. Retaining an old reference retains
its allocation, preventing pointer-reuse ABA. Node membership and parent/index
placement live in one generation-aware sparse directory owned by the graph.
Insertions, shifted siblings, detachment, collapse and flattening update their
actual placements before another parent query can observe them.

Source and target references are checked independently against the actual
current candidate. Stored WorkspaceIndex references cannot acquire fresh
authority by matching a version or by capturing from an independently built
equal Workspace. The concrete retained-Select lane records only its weak
predecessor and selected tabs identity. Real geometry/presentation fingerprints
remain separate data. Target surface/central/rule fields still exist and are
compared against fresh actual target facts; they are not unforgeable fields.

This is a breaking change for predicted-branch command capture. The replacement
`WorkspaceTransaction::apply_with` captures each next command from the same
actual candidate through a narrow `TransactionCapture` facet. It exposes no
mutable or canonical Workspace. Literal batches and incremental stages share
one executor and finalizer. The first push failure poisons the stage even when
the editing closure ignores that Result. Pruning, canonicalization and item
reconciliation still occur once at the batch end, and publication moves the
actual candidate. A net-zero Select batch has a false durable change report
while retaining the deliberately stricter new issuance.

The runtime layout compiler reserves direct child slots at the parent and fills
them during each child Enter. It removes the production visited map, optional
state conversion, second split lookup at Exit and child hash join. The raw
diagnostic compiler remains test-only; its DAG/cycle and error-order behavior
stays covered. The frozen solver/reference, numeric accumulation, complete rich
outputs, identity-sorted query directory and old-output ownership are retained.
Root ownership and linear compilation were frozen together; this checkpoint
does not claim independently measured speedups for those two components.

## Measurement

The comparison uses the unchanged 16/128/1,024 total panes over one/four logical
views, resize, center, perpendicular edge and supported cross-view operations.
It includes request capture, checked mutation, every root's cold layout and a
fresh complete owned export. ImGui remains pinned to
`d88f238d1d363ef387382b2e29ad12cf96f826ac`. Each comparison runs two serial
ABBA/BAAB groups, 48 samples and eight warmups per cell. Both full state vectors
and sampled digests must agree. Only the existing test work ledger is disabled
for frozen timing builds; ordinary verification retains it.

Both frozen comparisons finish with equal complete state vectors and unchanged
executables. All 42 group/cells improve p50 against `91f42ce`, by 13.20–69.37%.
At 1,024 panes, center improves 53.29–60.43%, edge/cross improves 57.02–68.01%
and resize improves 33.16–37.16%. One p95 cell regresses 105.34%, and seven max
cells regress, with the largest max regression 312.85%. These tails remain in
the published observations; the medians alone do not establish tail stability.

| Total panes | Complete-operation p50 / ImGui, all cells and groups | Cells at or below 2x |
| --- | --- | --- |
| 16 | 4.73–9.27x | 0/14 |
| 128 | 2.98–4.92x | 0/14 |
| 1,024 | 2.39–4.25x | 0/14 |

At 1,024 panes, resize remains 2.74–3.21x ImGui, center 3.56–4.25x,
perpendicular edge 2.39–2.79x and cross-view move 3.59–3.75x. Thus no cell has
reached the target. The small-pane fixed costs also need attention.

The first paired group's 1,024-pane phases against `91f42ce` locate the next
work: current move request p50 is about 0.17–0.21 microseconds, mutation
76–79 microseconds, layout 129–147 microseconds and export 51–75 microseconds.
These are separate phase medians, not an additive budget obtained by
subtracting independently paired percentiles. Candidate copying, cold leaf
materialization, repeated durable equality, generic solver passes and keyed
complete-output reads remain real implementation choices to test separately.

## Verification and known negative results

Counters-enabled verification passes 2,047 production core tests, 2,223 workspace
tests, four no-default-feature public integrations, the four targeted diagnostic
fixes, formatting, strict documentation, doctests, dependency boundaries and
correctness/suspicious Clippy. Three opt-in probes are excluded from ordinary
suite counts. The normal work probe is run separately. Independent root-owner
and layout source reviews find no remaining blocker in the inspected scope.
Formal source hashes must match these check receipts.

The matching ordinary docking matrix performs zero root fingerprint builds or
node visits and zero item-multiset scans or visits. Move still makes one complete
candidate clone, including 2,047 nodes/2,048 items for 1,024 panes in one view and
2,044 nodes/2,048 items in four views. Resize makes no transaction clone. These
counts do not describe cold creation, general canonicalization or scene geometry.

Cold/general completion still takes temporary before/after root values to
classify actual changes. An early implementation accidentally repeated that
work over all roots per recovery command; the unchanged 1,024-root test was
stopped after 118.593 seconds. The repaired exhaustive command match limits
each command to its affected roots, retaining the one final whole-forest pass.
The targeted suite subsequently passes 84 tests. The production recovery scale
case is still about 0.047 seconds versus 0.026 seconds at `91f42ce`, a negative
result requiring follow-up rather than a solved cold path.

The first full-workspace run had four failures. Two asserted obsolete work
counts; corrected expectations retain the real cold snapshots, explicit command
and final node-count checks. The tab-press case now verifies its actual zero
proof work while retaining clone/arming/release behavior. A frozen-close fixture
which had edited raw storage now uses a real checked Open; stale-source and
rollback assertions remain. The final complete suites pass, and historical
failure/stop logs remain part of the evidence.

The previous runtime head `91f42ce` passes all six CI jobs in
[run 42270945188](https://github.com/Latias94/egui_docking/actions/runs/42270945188).
This checkpoint requires its own pushed-head CI result.

The [evidence archive](2026-11-07-owner-reference/evidence.tar.xz) contains 436
input files, 16,128 raw rows and 336 complete state vectors. Its manifest and
all members were read back byte for byte. It includes source/binary receipts,
ordinary checks, historical failures, independent reviews and reconstructible
patch/new files. [Comparisons](2026-11-07-owner-reference/comparisons.csv) retain
every p50/p95/max cell; [SHA256SUMS](2026-11-07-owner-reference/SHA256SUMS) records
the published artifact hashes. Executables remain local and are identified by
hash rather than committed.
