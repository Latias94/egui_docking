# Design review: candidate-bound checked Move

Base: `a2bcabc1440caa664fb5f4a64c4669c4eecfdce4`. Read-only review of the proposed `operation`/`workspace` change. Only this report was written; no source edits, Cargo, benchmark, Git write or child agent was used.

**Decision:** use the existing `WorkspaceRootIndex` through private command-scoped `WorkspaceIndex::from_reference` / `include_reference` derivation, and a private `MoveOperation` that holds the exclusive candidate borrow until consumed. This is already a small, deep Interface. Do not introduce another scope-authority map, `RefCell`, a version-based cache, or a borrowed-fingerprint equality implementation in this slice.

The Module's Depth is one owner performing proof checks, policy and execution for one actual command. Its Seam is the exact candidate borrow. Locality improves because callers cannot separate successful checking from the candidate subsequently mutated. All dependencies are in-process. Existing egui/headless Adapters remain above this Module; no new Adapter trait is needed here.

## Proposed small Interface

The following is illustrative private Rust, not a request to expose new public entry points:

```rust
struct MoveOperation<'candidate, 'command> {
    candidate: &'candidate mut Workspace,
    payload: &'command MovePayload,
    target: &'command DockTarget,
    // Only execution operands that actually avoid repeated discovery, if needed.
}

impl<'candidate, 'command> MoveOperation<'candidate, 'command> {
    fn new(
        candidate: &'candidate mut Workspace,
        authority: TransactionAuthority<'_>,
        payload: &'command MovePayload,
        target: &'command DockTarget,
    ) -> Result<Self, CommandError>;

    fn apply(self) -> Result<AppliedCommand, CommandError>;
}
```

`new` borrows `&*candidate` while deriving and using the scoped index; that immutable borrow ends before returning the exclusive owner. `apply(self)` mutates the same candidate immediately. Fields stay private; do not implement `Clone` or expose the candidate between checking and consumption. Do not return a free-floating checked flag or an owned token usable with another workspace.

A call in the transaction command loop becomes conceptually:

```rust
let applied = MoveOperation::new(&mut candidate, authority, payload, target)?.apply()?;
```

The next command creates a new operation and fresh scoped facts against the state left by this command. A candidate lifetime is sufficient here; no epoch/revision number authorizes it. If compatibility with the existing `WorkspaceIndex` type requires a version field, an initial value is an implementation detail confined to this local owner. No caller can supply that number as proof of freshness.

Keep at most two private derivation operations on the existing index:

1. `from_reference`: preflight the source reference, derive that root's existing `WorkspaceRootIndex`, and establish the scoped facts.
2. `include_reference`: preflight a target reference and derive its root only if it is distinct. An already included root still requires validating this specific node's membership and this command's expected fingerprint; “root present” does not mean “reference accepted.”

The exact parameter list should reuse existing `RootId`/`NodeId`/`ReferenceRole` or reference types. No parallel root-index format, generic cache protocol, builder of arbitrary root sets, or persistent arena is warranted. Source/target re-capture should use the scoped facts directly; calling the standalone live captures inside this scope would recreate the deleted broad work.

## Comparison with allocation-free borrowed equality

| Choice | Benefit | Remaining cost / correctness work | Assessment |
| --- | --- | --- | --- |
| Command-scoped existing root indexes | Reuses the existing exact fingerprint, membership, item-count and owner representation; gives one derivation per actual root; supports source validation and target-fact rederivation together. | Allocates current root snapshots and membership sets once per distinct root. Ordinary and recovery error order must be preserved. | Recommended smallest slice. |
| Borrowed comparison of current workspace with captured fingerprint | A successful equality check may avoid materializing a second current snapshot and its Arc slices. | Must prove complete root coverage, membership and current presentation, not just compare whichever records the supplied snapshot contains. Must preserve malformed/missing/outside errors, cycles, exact selected/MRU/weight-bit semantics and complete `StaleNode { expected, actual }` diagnostics. A mismatch may still need actual fingerprint materialization. | Viable later internal Implementation, but do not describe it as universally allocation-free or add it beside scoped indexing now. |

Simply iterating the expected snapshot and finding matching live rows is not by itself a proof that the complete current root is equal: omitted/repeated rows, unreachable records and root ownership require handling. Nor should the fast path weaken full-root staleness into “the moved item is still present.” No measured removable budget for fingerprint allocation alone has been isolated. The first slice should remove repeated derivation while keeping the current exact value representation and existing counter meaning.

## Validation and error order to preserve

The ordinary Move path currently checks arrangement lock before source validation (`operation.rs:717`), then source proof/type/content, target proof/facts, move semantics and policy (`:908`, `:971`), then applies. Preserve that observable sequence when combining the phases.

| Stage | Required order / invariant |
| --- | --- |
| Ordinary lock | `ArrangementLocked` precedes stale/missing source when arrangement is locked. The recovery branch does not run this gate. |
| Source basic reference | `MissingRoot` before `MissingNode`, then `NodeOutsideRoot`, then fingerprint inequality (`StaleNode`). After the reference check, retain `NodeIsNotTabs`, `ItemNotInTabs` or empty payload checks as appropriate. |
| Target derivation | Reach this only after complete source validation. For an edge, `EdgeTargetScopeMismatch` currently precedes target reference lookup; check scope before eager `include_reference`. Then preserve missing/outside/stale order. |
| Frozen target facts | Recompute surface owner, central flag and pane-local selected-item rule from the current candidate/scoped facts, then compare the complete target. A valid fingerprint does not authorize caller-forged `surface`, `central`, `rule` or scope. |
| Target shape | Center/TabGap still require tabs; TabGap index bounds follow frozen-target-fact validation. Outer-edge target still denotes the actual current root node and root rule, not an arbitrary branch relabeled as outer. |
| Ordinary move semantics | Preserve payload-containment, split-payload-to-tabs, central-detachment and edge-weight checks before ordinary drop policy. TabGap also retains its tab-bar policy check. |
| No-op execution | Same-tabs center and exact same-branch inner-edge no-ops still return the existing changed flags and avoid inappropriate detach/reinsert. Do not convert a valid no-op into an insertion or alter the selected/MRU outcome. |

An eager all-root `WorkspaceIndex::build` is not a substitute for this source-first scope: it can both visit unrelated roots and return an unrelated owner/graph error before the command's source error. The root's proposed preflight of missing root/node/outside **before first root construction** avoids that change. For malformed raw drafts, retaining that first membership check also prevents root-index construction errors from jumping ahead of `NodeOutsideRoot`. Once the root is derived, additional membership checks use its actual members.

Within one same-root command, validate source and target against the same derived current snapshot, even when their expected fingerprint Arcs differ. Never use Arc identity alone to accept them. Same-root does not imply equal captured values: a source can be current while a target carries an earlier selected/MRU or owner value.

## SurfaceRecovery is a real Move caller

Do not assume recovery only rehomes roots. `surface_recovery.rs:402` admits a complete-main `Move`, and `:1166` compiles real Tabs/Subtree Moves to a dock target.

`prepare_surface_recovery_transaction` (`operation.rs:610`) first verifies the exact recovery program and complete roster. Its `TransactionAuthority::SurfaceRecovery` skips application policy, but current `apply_command`/`move_payload` still validates source/target proof and execution structure. Preserve that distinction:

* Do not route recovery through `authorize_workspace_command` or enforce arrangement lock, source restrictions, tab-bar rules or ordinary drop policy.
* Do not turn policy bypass into proof/shape bypass. Missing/outside/stale sources and forged frozen target facts still fail.
* Preserve recovery's existing structural/error ordering. It currently skips the ordinary `DropCommandEligibility::check_target` pre-policy semantic pass and reaches application checks/no-op handling directly. Adding that whole ordinary pass can change error priority or no-op behavior even if policy itself is disabled.
* Keep the authority enum private and reachable only after exact-program/roster validation. No generic public `skip_policy: bool` or caller-selected “trusted move” mode.

## Caller migration and deletion

1. In `prepare_transaction_inner`, dispatch Move through the checked owner instead of separately calling authorization then the old revalidating Move arm. Keep original command index mapping, typed item delta, final canonicalization, item reconciliation and publication placement.
2. Route `apply_owned_draft_command` through the same Move owner. Otherwise the second durable mutation path retains duplicated validation or diverges in semantics.
3. Keep the existing read-only `authorize_workspace_command` Interface. `engine.rs:3376` uses it when committing a prepared drop candidate; product actions and native lifecycle preflights also use it. Those checks are real and must not be deleted merely because ordinary preparation becomes combined. Its Move branch can reuse the private scoped checker while returning `()` and granting no durable execution capability.
4. Keep `DropCommandEligibility::new_indexed` / `check_target_indexed` for the existing many-target drag resolver (`drop_resolver.rs:811`). That read-only indexed use is distinct from an exclusive candidate command. Do not force every hovered target through a fresh two-root command scope.
5. Delete the old unchecked/revalidating Move entry once its only execution path is `MoveOperation::apply`. Move the proven raw mutation body into that consumed implementation or keep a module-private helper whose only call is there. Do not leave an alternative durable path that bypasses checking or repeats the old live proof construction.

This increases Depth without expanding the callers' Interface. No transaction storage/rollback redesign is needed in this slice. The separate traversal lane can remove Node/vector copies without changing this command owner.

## Behavior tests to lock before measuring

Use meaningful parameterized cases around the actual transaction Interface; keep tests focused rather than adding a proof-testing framework. Existing tests can be extended where fixtures already cover the condition.

| Behavior | Necessary assertion |
| --- | --- |
| Same-root Item center and perpendicular edge | Exact membership, tab order, selected/MRU, node/root ownership and changed outcome match existing semantics. Scope derives exactly one root. Include a same-tabs no-op. |
| Cross-root Item/Tabs/Subtree where legal | Both roots, surviving panes/central metadata and item multiset are correct; scope derives exactly two roots. Unrelated roots are not fingerprinted by the command. |
| Source and target errors | Table covering missing root, missing/recycled node, existing node outside claimed root, stale values and wrong node kind. Check `ReferenceRole`, source-first error and original workspace unchanged. |
| Same-root different captured values | Current source plus stale target fails as target `StaleNode`; stale source plus another invalid target fails at source. Do not accidentally accept because both reference the same RootId or one cached Arc. |
| Forged target policy fields | Change surface, central or rule while preserving a real fingerprint; reject through the same frozen-fact diagnostic. An outer target relabeled from a branch and an edge scope mismatch also reject. |
| Policy priority | Locked + stale ordinary command still reports arrangement lock. Unlocked disabled source/target/operation and hidden TabGap reject without mutation after structural facts are verified. |
| Recovery | A valid exact recovery with real Tabs/Subtree Move succeeds despite ordinary arrangement/drop restrictions. A stale target or forged target fact still rejects. Existing injected-program rejection remains before mutation. |
| Sequential batch freshness | `[Select(changing), Move(old_same_root_proof)]` fails at command 1 and rolls back command 0. An unchanged first command preserves a later valid proof. Separate commands on independent roots use their staged current state; never reuse the initial batch snapshot wholesale. |
| Late failure and discard | Existing Move followed by invalid contained geometry still rolls back the complete transaction and remains retryable (`transaction.rs:227`). Existing prepared candidate can still be discarded/retried (`:175`). |
| Diagnostic/no-op shapes | Preserve source-inside-target and split-to-center errors, central detach rejection and exact no-op outcomes for Tabs/Subtree. Recovery structural order merits one focused counterexample, not just an Item benchmark. |

For isolated operation-level counters, expect one current-root build for a same-root Move and two for a cross-root Move, with node visits equal to the sum of actual referenced root sizes. The renderer-free harness still builds its separate caller request index: its complete counted step should therefore become `V+1` builds / `N+n` visits for same-root and `V+2` / `N+2n` for equal-sized cross-root cases. These are falsifiable source predictions, not measurements or speedup claims. Recovery's complete-roster preflight has its own captures and must not be confused with this per-command count.

## Review gates

The selected design is acceptable if each actual command validates full current facts once per referenced root, forged target facts still reject, policy-independent recovery keeps its limited authority, and every batch command reconstructs scoped facts against the then-current candidate. Reject a design that caches by version, returns an executable token detached from the candidate borrow, builds unrelated roots, drops the prepared-release read-only authorization, or exposes an unchecked mutation path. No broader refactor is requested by this review.
