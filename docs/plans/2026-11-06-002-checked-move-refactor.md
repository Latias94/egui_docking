# Validate a docking move once per candidate state

Date: 2026-11-06. Baseline: `a2bcabc1440caa664fb5f4a64c4669c4eecfdce4`.
Status: implemented and reviewed; Move timing criterion met. Small resize
median/tail limitations remain disclosed in the [results](../performance/2026-11-06-checked-move.md).

## Problem and result

The matched renderer-free docking benchmark attributes about 73% of four-view
move time to checked transaction publication. A single move currently validates
source/target during authorization, re-captures the target to compare policy
facts, and repeats both validations during execution. With the initial request
index this builds V+6 complete root snapshots. Canonicalization then clones each
Node on entry/exit, and strict validation clones it again.

The result of this slice is one candidate-bound Move preparation that derives
current source/target facts once, checks real policy and move semantics, then
consumes those facts during execution. Only distinct referenced roots may be
materialized inside that preparation. Nothing survives into the next command
or becomes a persistent authority cache. Canonical and validator traversals
borrow immutable records, obtaining owned contents only when an actual rewrite
needs them.

## Design and migration

- Put preparation and execution under one private operation owner. The caller
  supplies the exclusive mutable candidate; the owner checks and consumes a
  command without exposing an unchecked executor or an authorization token
  that can be paired with another workspace/command.
  The implementation uses one fused private function rather than introducing
  an intermediate checked-token type: its immutable checking borrow ends
  immediately before mutation under the same exclusive candidate borrow.
- Reuse existing current-root/index extraction and policy fact definitions.
  Prefer a small scoped derivation over adding another proof schema, generic
  transaction context, revision cache or mirrored docking implementation.
- Preserve authorization error ordering, arrangement lock, actual target scope,
  selected-item policy identity, central restrictions and source membership.
  Native surface-recovery authority skips policy only for its exact program;
  it still needs structural preconditions.
- Build current facts from the actual staged candidate for every batch command.
  A prior same-version edit or earlier command must invalidate a stale capture.
  Do not trust the caller's request index as publication-time authority.
- Keep the one full workspace candidate, all item-conservation checks and final
  canonicalization/strict validation in this slice. Removing broad passes or
  replacing rollback storage requires a separate invariant-owner design after
  the measured result. Do not weaken guarantees to meet a timing target.
- Delete obsolete repeated Move validation paths when the new owner covers all
  callers. Migrate transaction preparation, owned-draft follow-ups and standalone
  authorization directly. Keep one implementation for real policy/semantics.

The initial numeric version stored by a command-local index only satisfies the
existing index Interface. Freshness comes from deriving facts from the actual
borrowed candidate at each call and comparing every expected fingerprint,
including two different expected values for a same-root source and target. No
version number is accepted as authority for internal same-version edits.

## Verification and investment criterion

Existing public stale-reference, batch atomicity, policy, central/node payload,
same-pane/no-op, native recovery and canonical graph tests remain authoritative.
Add focused regressions where the new lifetime/reuse seam has no coverage,
especially late staged-state failure and independent frozen target facts.
Use the work ledger to verify that a move builds at most one current snapshot
per referenced root, rather than the previous six internal captures.

Freeze the unchanged baseline executable before edits. Compare baseline and
candidate on the same complete request + checked mutation + all-root layout +
fresh owned export workload at 16/128/1,024 panes and one/four views. Two serial
ABBA/BAAB groups must retain all raw rows, full state agreement and tail values.
At 1,024 panes, center-move p50 should improve at least 20% in each group;
edge/cross-view must move in the same direction, with no unexplained repeated
resize regression. This is an investment criterion, not a prediction that the
remaining gap is irreducible.

Root serializes Cargo, nextest and measurements with one build job. Run ordinary
tests without the compile-time benchmark ledger bypass. Qualify applicable core
and public integration tests, fmt, correctness/suspicious Clippy and dependency
checks. Independent implementation and evidence reviews must close blocking
findings before a precise Conventional Commit and push. User CI/review/reference
changes are excluded and their starting hashes are preserved.

## Measured first iteration and bounded follow-up

The first implementation passes 2,021 all-feature core tests and the separate
21-cell work-count probe. Independent review finds no blocking issue. Its
frozen comparison improves 1,024-pane one-view center p50 by 34.36%/35.80%, but
four-view center improves only 19.14%/18.79%, below the 20% investment criterion.
Keep those raw samples and source/executable identities; do not redefine the
criterion or replace that result with a static work-count claim.

The second bounded increment removes two remaining allocation patterns in the
same actual success path, without deleting validation:

- Replace full item collection used only for emptiness with a cycle-safe,
  iterative early-exit boolean query. Central roots need no emptiness query at
  all. Full inventories used by policy/conservation remain complete.
- Check exact MRU permutations directly for empty/single/two-item stacks.
  Preserve length, selected-first, uniqueness and complete membership. Larger
  stacks retain the existing set implementation. Strict validation returns
  early only for a completely valid short MRU; invalid inputs retain the full
  existing ordered diagnostics and global ownership checks.

This does not introduce a configurable cutoff, repaired input, weaker canonical
invariant or long-stack algorithm replacement. Concrete invalid/valid regression
tables and independent incremental review accompany it. Re-run full tests and
freeze a distinct candidate before repeating both formal comparison groups.

The final increment passes 2,023 all-feature core tests, four public-facade
integration tests and nine documentation tests. Center-move improvements are
43.92%/42.79% at one view and 27.97%/27.05% at four views; edge and cross-view
also improve in both groups at every size. All normalized output is unchanged.
The separate resize follow-up does not repeat the four-view median regression,
but one-view medians remain 2.08%/0.40% slower and tails are mixed. Those smaller
changes are not assigned a proven cause or hidden by the Move qualification.
Whole-workspace rollback and the four global item scans remain the next design
problem, rather than being removed without an invariant-owner argument here.
