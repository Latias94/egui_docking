# Final bounded simplification review

Reviewed the current runtime diff in `operation.rs`, `workspace.rs`, `graph.rs`, `canonical.rs`, `validation.rs`, the relevant call sites, the new focused test source, and the checked-move plan. The scope is delivery of the frozen bounded feature, not another optimization pass. No source or plan was edited; no Cargo, tests, benchmark, profiler, Git write or child agent was used. Root's reported 2,023 passing all-feature tests / three skips were not rerun by this reviewer.

**Verdict: no confirmed correctness or scope issue; no code change required before completing the frozen comparison.** The implementation remains within checked Move derivation, borrowed traversals, and two small success-path queries. It does not redesign rollback, delete global validation, or add a general proof/cache framework.

## Scope and single owner

`WorkspaceIndex::for_command_reference` and `include_command_reference` (`workspace.rs:84`, `:99`) have only the intended runtime caller, private `check_move` (`operation.rs:1038`). They reuse the existing root-index representation, derive only actual referenced roots, and add no persistent storage. The source-first inclusion and actual-membership checks are retained. The initial numeric version is internal compatibility data, not caller-provided freshness authority.

Their `pub(crate)` visibility serves the existing sibling-module call from operation to workspace; making them more local would require moving or wrapping implementation across that Seam. There is no demonstrated scope benefit worth changing the frozen code. The actual lifetime is local and clear from the only caller.

Durable Move has one private executor: `apply_command` -> `move_payload` -> `check_move`, then immediate mutation under the same exclusive candidate. No checked token, persistent scope or unchecked alternate executor exists. The transaction loop and owned-draft follow-up both dispatch through it. Next batch commands create new scopes from then-current candidate state.

The surviving `authorize_move` -> `check_move` path is read-only preflight used by actual engine/product callers. It is not a second mutation executor. Indexed many-target `DropCommandEligibility` remains used by drop resolution. Its removed live constructor and live target-check method have no callers left. Remaining live reference validation is still used by non-Move commands and must not be removed as apparent Move duplication.

The duplicated execution-time subtree containment/split-to-tabs block was removed from `move_payload`; its shared semantic check precedes mutation. Exact self-center/no-op handling remains in execution. Previously reviewed recovery constraints still hold, and the new `exact_main_move_recovery_bypasses_policy_but_rechecks_its_target` test now directly covers both Tabs/center and Subtree/edge recovery, ordinary-policy rejection, stale target rejection, item preservation and complete publication.

## Small MRU helper is exact and bounded

`is_exact_tab_mru` (`graph.rs:15`) keeps the original predicate. Its preliminary length and selected-first comparison applies to every branch:

| Item count | Exact remaining condition |
| --- | --- |
| Zero | Both slices empty; selected must already be None. |
| One | The sole visual item equals the sole MRU item; selected already equals that MRU item. |
| Two | Visual members are distinct, and MRU is either complete ordering of those two members; selected already equals its first member. |
| Larger | The existing set equality and unique-MRU predicate remains unchanged. |

The two-item branch also excludes duplicate MRU members: neither complete ordering can hold for a distinct visual pair if the MRU duplicates one value. Missing MRU remains invalid. This is a short exact predicate, not a configurable threshold, a repaired-input path, or a replacement long-stack algorithm.

Strict `validate_tab_mru` (`validation.rs:604`) returns early only when a short stack passes that complete predicate. Invalid inputs retain the original detailed checks and ordered errors. `validate_tabs` still checks noncentral empty leaves/selection before it and performs global duplicate-item accounting afterwards. Thus a valid local MRU does not bypass graph, ownership, selection or presentation validation. The small helper table and strict diagnostic-order test sources cover the changed predicate and failure path.

No extraction of a general membership engine, additional validation mode, or test protocol is justified here.

## Emptiness and borrowed traversal stay within scope

`Workspace::subtree_has_items` (`workspace.rs:1012`) is the boolean equivalent of `!collect_items_in_subtree(...).is_empty()`: it uses the same explicit stack, visited set, child order, missing-node behavior and cycle handling, and may return as soon as a nonempty leaf exists. Its only callers need that boolean. The complete item collector remains for real inventories and policy/conservation work.

Checking the central-root flag first in cleanup avoids an unused query without changing an error result: the former collector was also infallible and returned no diagnostic. `ensure_nonempty_payload` preserves its `EmptyPayload` result. The helper is appropriately small; no generic traversal callback/iterator is needed for two distinct output needs.

Canonicalization and strict validation now borrow Node records instead of cloning full vectors. Traversal stacks still contain IDs/control frames, not references carried through mutation. Canonical rewrite scans all required child results before its unchanged return; changed splits still construct and normalize replacement storage. Full input/output validation, reachability, conservation and publication checks remain. These are concrete deletions of temporary copying, not a broadened incremental-validation or rollback project.

## Comments and optional documentation

The local comments state necessary ownership/error/numerical constraints: dispose the scoped index before mutation, check and execute under the exclusive candidate, and preserve unchanged split weight bits. They are useful, not redundant narrative. No comment cleanup is necessary during the freeze.

**Optional P3 documentation only, after the frozen evidence is complete:** the general `WorkspaceIndex` type comment (`workspace.rs:45`) emphasizes an exact revision and scene-wide derivation, while a command-local instance now intentionally contains only referenced roots and uses an internal compatibility version. A short qualification could prevent a future caller from treating every index as a complete root roster or its version as sufficient authority:

> Scene indexes cover their revision's roots. Command-local indexes contain only the roots derived during one unchanged candidate check and never survive mutation.

The checked-move plan and helper comments already explain this distinction, so this is not a blocker or a requested runtime change. No other simplification change is necessary for this slice.

## Disposition

Proceed with the frozen result and bounded feature delivery. No new micro-optimization, helper relocation, cache abstraction, traversal framework, validation deletion or rollback-architecture change is recommended by this review. The only suggestion is the optional type-documentation qualification above.
