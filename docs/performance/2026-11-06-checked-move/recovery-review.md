# Fused Move diff review: recovery and scoped references

Reviewed the actual current `git diff` for `operation.rs` and `workspace.rs`, the new checked-move test source, and existing recovery construction/contract tests. This is a static review; no source edits, Cargo, tests, timing, benchmarks, Git writes or child agents were used. Source anchors refer to the current implementation inspected on 2026-11-06.

**Verdict: no confirmed blocker.** The fused private `move_payload(&mut Workspace, Option<&DockPolicySnapshot>, ...)` is smaller than the illustrative checked-token design and retains its relevant ownership property: it performs `check_move`, then immediately executes against the same exclusive candidate. The scoped index is local to the immutable check and is gone before mutation. No token, scope index or unchecked alternative executor escapes across a mutation or a batch command.

The new policy-free semantic precheck does not change accepted behavior or no-op behavior for a valid, compiler-produced exact `SurfaceRecoveryTransaction`. Source/node/membership error order and same-root expected-proof comparisons also remain intact. No workspace-scope order fix is indicated by this diff.

## Why exact recovery excludes the concerning branches

These are actual constructor/application constraints, not assumptions made from the benchmark fixture:

1. `surface_recovery.rs:372` requires the destination surface to differ from the source roster surface.
2. `:384` places the optional main-root command first, followed by the exact complete contained-root roster with no extra commands.
3. For a Move, `:400` requires a Tabs/Subtree source exactly equal to the captured complete main `NodeSource`. `SurfaceRosterDisposition::capture` captures the main at its actual root node (`:690` onward); `compile_main_rehome_command` at `:1154` chooses Tabs for a tabs root and Subtree for a split root.
4. `operation.rs:610` still checks exact program shape and the current complete source roster before preparing any command. `Workspace::matches_surface_roster_source` at `workspace.rs:547` compares current source values.
5. `check_move` validates the actual target and rederives its surface/central/rule facts (`operation.rs:1072`–`:1085`). A forged `target.surface` cannot make a target on the source root appear to be a legitimate cross-surface destination.

For a valid forest, source and validated target therefore belong to different roots. The source node is its complete root node. This excludes the ordinary same-root behaviors that originally motivated caution:

| Added precheck branch | Consequence for exact recovery |
| --- | --- |
| Target inside source subtree | Impossible for a correctly validated target on another surface/root. The earlier application branch also rejected it. |
| Self-center / same-branch no-op | Impossible across different validated root owners. Forging target surface fails frozen-fact validation before reaching a no-op. |
| Partial central subtree detachment | `source_record.node == source_node`, so `validate_move_target_semantics` at `operation.rs:1347` skips this branch. Complete-root transfer still preserves the existing execution rules. |
| Split payload into a center/TabGap tabs target | Already rejected before detach by the old `move_payload`. The new helper returns the same `SplitPayloadIntoTabs` in the same relative position after source/target validation. |
| Edge insertion weight | The source root detaches from a different root; target parent, children and weights remain unchanged. See the numerical comparison below. |

No proof/type/content validation is bypassed when `policy == None`. Only the `DropCommandEligibility::new_validated(...).check_validated_target(...)` call is conditional (`operation.rs:1087`). `apply_command` still runs arrangement/non-Move policy only for `TransactionAuthority::Policy`, and maps the recovery authority to None solely inside the private Move dispatch (`:1421`). Exact-program and roster authority remain the entry gates.

## Edge weight precheck is equivalent for this caller

`validate_edge_insertion_weight` at `operation.rs:1362` checks the same f32 operations that actual insertion uses:

* When target parent axis matches the edge axis: compute `payload = branch*fraction`, `target = branch-payload`, validate both positive finite weights. Actual `split_existing_branch_roster` performs the same operations (`:3141` onward in this checkout).
* When there is no same-axis target parent: validate `fraction` / `1-fraction`. Actual `wrap_target_branch` uses those same values.

For ordinary same-root Tabs moves, detach may change target weights, so actual insertion must continue to calculate weights after detachment. The refactor does keep that calculation; the new `moving_sibling_tabs_uses_parent_weights_after_detach` test covers a representative case.

For exact recovery the source/target roots differ, so that intervening change cannot occur. An underflow/rounded-zero `InvalidEdgeWeight` that previously arose during insertion is now rejected before detachment with the same axis error. Externally, the transaction still rejects without changing the workspace. There is no valid recovery in which a previously invalid target branch becomes valid because the complete source root was detached elsewhere.

For deliberately malformed internal transactions/drafts with multiple simultaneous faults, moving structural checks earlier can change which fault is reported before a later detached-state failure. This review does not claim universal diagnostic identity for arbitrary forged recovery programs. The exact-program/roster/target gates exclude the relevant accepted/no-op counterexamples on a valid baseline, and no broader bypass Interface was introduced.

## Scoped reference order and same-root values

`workspace.rs:99` `include_command_reference` follows:

```
MissingRoot
    -> MissingNode
    -> NodeOutsideRoot
    -> build current root once if absent
    -> indexed membership check for this node
```

For a first root, it uses actual `root_contains_node` before construction, matching the old standalone capture's error precedence. For an already included same root, it still checks root existence and current node existence, then validates this node against the scoped root's actual membership set (`workspace.rs:315`). No mutation can intervene while `check_move` holds the workspace borrow, so the two membership mechanisms agree for the current candidate. An existing foreign node is not accepted just because the root already has an entry.

The complete expected fingerprint is checked **per reference**, not per root-entry insertion. `WorkspaceIndex::verify_reference` at `workspace.rs:337` compares the scoped actual fingerprint to the supplied source or target expected value. Thus a current source plus a stale same-root target still fails as target `StaleNode`. Sharing the actual root does not share an acceptance decision about expected values. The new `current_same_root_source_does_not_accept_an_older_target_snapshot` test exercises exactly this distinction.

`check_move` finishes complete source proof/type/content validation before target scope/inclusion. It checks edge scope mismatch before target root/node lookup (`operation.rs:1058`), preserving source-first and scope-first target diagnostics. Target re-capture uses indexed owner/central plus current pane-local rule, so the full target equality still rejects forged surface, central or rule values. The new parameterized forged-target test covers all four target variants and these fields for ordinary transactions.

The index's `WorkspaceVersion::initial()` value is merely an internal compatibility field for existing indexed methods. It is not an external freshness test. `check_move` constructs the index from the current workspace on every call; the command loop calls `apply_command` afresh for every staged command (`operation.rs:657`). No cached index is carried to the next command.

## Relevant behavior tests and remaining focused coverage

Source inspected, not executed by this reviewer:

* New tests cover selection before target/source use, all payload/target variants, prior Move changing a later target, current-source/stale-same-root-target, forged target facts, no-op TabGap policy, one/two distinct-root derivation counts, and post-detach sibling weights.
* Existing `surface_recovery::rehome_can_dock_the_exact_complete_main_payload` (`surface_recovery.rs:1758`) covers a real complete-main Tabs Move and complete contained roster publication.
* Existing recovery injection, stale roster and late final-member rollback tests protect the typed recovery gate and atomicity.
* Existing `copied_recovery_commands_cannot_forge_typed_policy_authority` (`:2019`) uses a **rootless** fixture. It proves policy authority for RehomeRoot recovery but does not specifically exercise the new Move/None-policy branch. Do not cite that test as direct coverage of this branch.

Three small additions give the most direct coverage without another framework:

1. Extend/reuse the real complete-main Move recovery fixture with ordinary arrangement/drop policy disabled. Copied ordinary commands must reject, while the typed recovery still publishes exact members/owners. This distinguishes policy bypass from proof bypass on the actual changed branch.
2. Compile a valid main Move recovery, change only target-root selected/MRU state, then apply. The source roster remains current; the target proof must fail and the whole workspace remain unchanged. A forged surface/central/rule target with a valid source roster should similarly reject; one field is sufficient if the parameterized ordinary test remains.
3. One compact reference-error table for source and target: absent root, absent/recycled node, existing foreign node, and edge scope mismatch. Add a same-root included-source/foreign-target row, so the occupied-index-entry branch is exercised. Confirm roles and source-first/scope-first errors, not just `is_err()`.

A complete-root Subtree recovery to a valid edge is a useful additional acceptance case if not already present in the wider suite, especially with central metadata on the source. It is not evidence of a current bug: the static proof above shows the new partial-central check is skipped for a whole root.

## Disposition

Accept the fused Interface and the current scoped membership logic. No confirmed code correction is needed for legitimate exact recovery acceptance/no-op semantics or missing/outside ordering. Preserve the post-detach numerical insertion checks and the private exact-recovery authority gate. Use the focused tests above to make the static reasoning durable; no expanded refactor or allocation-free fingerprint scheme is requested.
