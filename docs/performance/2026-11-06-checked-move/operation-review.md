# Read-only review of candidate-local checked Move

Date: 2026-11-06. Reviewed the actual working diff in `crates/dockspace/src/operation.rs`, `workspace.rs`, and the complete new `operation/tests/checked_move.rs`. No source changes, Cargo, tests, benchmarks, or Git writes were performed. This is a source correctness/architecture review, not a test-pass receipt. Root retains source and execution ownership.

## Disposition

**No blocking correctness defect found in the inspected execution paths.** The private function owner is sufficient for this slice: `move_payload(&mut Workspace, ...)` checks the exact command against that candidate, the command-local immutable index is destroyed before mutation, and no unchecked executor/token or retained parent/index/weight cache escapes. No extra operation object or authority framework is needed to encode the present synchronous call chain.

One focused behavior test is still recommended before treating the new recovery Move seam as directly covered: valid complete-main dock recovery under disabled ordinary policy, plus stale-target rejection through that typed recovery program. Existing policy-bypass tests largely exercise rootless RehomeRoot programs rather than this new `Move(..., None)` execution path. This is a coverage finding, not a demonstrated policy bypass bug.

## Unified apply authorization audit

`operation.rs:644`, `prepare_transaction_inner`, and `690`, `apply_owned_draft_command`, both now call the same `apply_command` with explicit authority. The search found no other direct caller of that private dispatcher.

At `operation.rs:1421`, `apply_command` has two ordinary-policy paths:

- Move checks arrangement lock before calling its private checked mutation owner. The owner performs all source/target facts, semantics, TabGap interaction, and actual drop-policy checks through `check_move(..., Some(policy), ...)`.
- Every other command still calls `authorize_workspace_command` before dispatch. Existing Select and RaiseContained lock exemptions remain in that authorizer; other operations retain their arrangement-lock, source/target and operation-specific checks.

Declared-open-item validation still precedes dispatch in both transaction wrappers, preserving its old placement. RootContent-based operations, resize, root rehome, contained transformations, and empty-container operations were not silently excluded from authorization by the Move special case.

The standalone read-only `authorize_workspace_command` Interface remains. Its Move branch uses `authorize_move -> check_move` and still enters through the author's arrangement-lock gate. Prepared-release and product/lifecycle preflights therefore retain their genuine policy check without exposing execution authority. Indexed `DropCommandEligibility::new_indexed` and `check_target_indexed` remain available for many-target drag evaluation, while only the obsolete standalone live constructor/check wrapper was removed.

## Source/target scope and independent expected snapshots

`operation.rs:1038`, `check_move`, derives source facts from the actual current candidate. `WorkspaceIndex::for_command_reference` (`workspace.rs:84`) and `include_command_reference` (`99`) build only roots actually referenced by this command. The sole callers are in this checked owner; there is no reuse into the next batch command.

Source validation finishes before target inclusion. It still checks source reference fingerprint independently from payload kind, item membership and nonempty payload semantics. For an edge, the explicit `EdgeTargetScopeMismatch` gate occurs before target lookup, matching the existing target validator order.

The target is **not** accepted merely because its root already exists in the scoped index:

1. `include_command_reference` checks root/node existence and indexed node membership even for an already included root.
2. `validate_target_with` invokes `WorkspaceIndex::verify_reference` (`workspace.rs:337`) with the target's own expected fingerprint and `ReferenceRole::Target`.
3. Tab/edge target rederivation then compares the complete frozen target facts against the current candidate/index: surface, central status, selected-item/root policy identity, root-vs-inner scope, edge and fraction.

One derived current snapshot can therefore validate two different captured values without treating them as equivalent. A current same-root source plus an old target still fails as target `StaleNode`. `current_same_root_source_does_not_accept_an_older_target_snapshot` directly locks this property. Missing/outside checks are independent of fingerprint equality; outer-edge rederivation produces the actual root node and root rule, so a branch cannot gain outer authority by changing only the enum/scope.

The index's initial version field is a local representation detail, not freshness authority: each `check_move` creates a new index, uses it under the same immutable candidate borrow, returns only `Result<()>`, and discards it. The following mutation uses the same private caller's exclusive candidate and exact payload/target arguments.

## Recovery with no application policy

`operation.rs:610`, `prepare_surface_recovery_transaction`, rejects non-exact programs before checking the complete frozen surface roster, and only then enters preparation with private `TransactionAuthority::SurfaceRecovery`. Ordinary callers cannot choose `None` policy or manufacture that authority through `WorkspaceTransaction`.

`surface_recovery.rs:372..435` restricts accepted recovery programs to a complete-main Move/RehomeRoot followed by the exact frozen contained-root rehomes. The target surface differs from the source. A Move must carry the expected complete-main Tabs/Subtree source and frozen target surface. Partial arbitrary item Moves, injected commands and copied ordinary commands do not acquire recovery authority.

For this authority, the dispatcher skips application policy and arrangement lock. `move_payload` passes `None` to `check_move`, which still validates source and target proof, target rederivation and structural move semantics; only `DropCommandEligibility`'s application-policy stage is omitted. TabGap policy is inside that omitted policy stage, so it is not accidentally reintroduced into recovery.

The refactor applies `validate_move_target_semantics` to recovery before detach, whereas old recovery execution performed its structural checks in the mutation body. I checked the actual admitted valid programs: their source is a complete root, the destination is a different surface/root, and no source-parent removal can change the destination's parent weights. Containment/split-to-tabs checks and target edge-weight validation therefore remain legitimate structural checks, not a policy gate. Partial-central-detach and same-root no-op cases are not accepted complete-main cross-surface recovery shapes. I found no valid exact recovery whose outcome changes because of this relocation.

The missing direct regression should use `SurfaceRosterDisposition`/`SurfaceRehomePlacement` with `SurfaceMainRehome::Dock`, rather than a generic forged policy flag or another rootless rehome. Assert the compiled program contains a real Tabs/Subtree Move, copied ordinary commands reject under lock/source/drop restrictions, the typed recovery still succeeds, and a separately changed target makes the retained program fail without publishing any source-roster change. Existing injected-program and complete-roster tests continue to cover the outer authority barrier.

## Detach-time addressing and branch weights

`check_move` returns no parent link, child index, or weight roster. Its edge-weight preflight (`operation.rs:1362`) checks the current structural operation but does not authorize a stored insertion offset.

Execution at `move_payload` (`1578`) checks before any no-op/detach path and then uses existing mutation helpers. `detach_node` (`2872`) obtains the live source parent. `remove_split_child` removes the source child and normalizes the remaining weight roster. Later `insert_payload_at_edge` (`3072`) obtains the target's **then-current** parent; `split_existing_branch_roster` (`3141`) locates the target's then-current position and reads that branch's current weight.

No pre-detach target index/parent/weight has been hoisted into the scoped index. The new `moving_sibling_tabs_uses_parent_weights_after_detach` regression is a meaningful adversarial example: `[0.2, 0.3, 0.5]` loses the first child, the survivor weights become `[0.375, 0.625]`, then the target share splits into `[0.1875, 0.1875, 0.625]` in the required visual order. The test would detect use of the old target index or old 0.3 share.

Same-tabs Item center/reorder and singleton edge no-ops still occur only after complete policy/structural checks. Group same-tabs center no-ops rely on already validated target shape; deleting the old redundant `matches!(Node::Tabs)` guard there is safe because `validate_target_with` has already required the target to be tabs. Exact sibling-edge no-op simulation still uses copied current rosters locally and does not persist them into actual detach/insert.

## Regression assessment

The new tests cover distinct risks without introducing a new proof framework:

- Earlier Select invalidates every target kind and each source payload kind with the correct command index/role and complete rollback.
- Earlier Move invalidates a later target against staged state, preventing batch-level initial-index reuse.
- A current source cannot launder an older same-root target snapshot.
- A correct fingerprint cannot launder forged surface/central/rule fields for any target kind.
- Same-tabs TabGap no-op still checks real tab-bar policy.
- Each successful Move derives one or two actual referenced roots and leaves the unrelated third root outside fingerprint work.
- Detach followed by same-parent insertion reads current normalized weights and index.

The counter assertions are scoped after fixture/request construction and before later validation, so they test internal command derivation rather than conflating the caller's request index with mutation work. They do not substitute for semantic assertions or rollback tests. The one remaining focused request is the typed recovery **Move** policy/staleness combination described above.

## Bounded compatibility note

On valid published workspaces and valid staged command candidates, source/target missing/outside/stale ordering is retained. In a deliberately malformed raw private Workspace, `include_command_reference` can attempt root snapshot construction before returning outside-root membership, whereas standalone live capture first walks membership. An unrelated corruption inside the claimed root could therefore change which error appears for such a fabricated internal state. I did not find a public/staged path that produces this condition: public workspaces are strictly built, and successful mutation helpers retain the referenced root's structural/MRU records until canonicalization. This is not a demonstrated blocker; if arbitrary malformed owned drafts are intended to have the same diagnostic-priority contract as published workspaces, that precondition and a targeted case should be made explicit rather than adding broad validation passes speculatively.

No test or performance result is claimed by this review. Root's ordinary production nextest and subsequent evidence remain the execution qualification.
