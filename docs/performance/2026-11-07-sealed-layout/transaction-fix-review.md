# Numeric frontier correction: independent source review

Date: 2026-11-07. Result: NO REMAINING BLOCKER FOUND in the inspected correction. The two P1 witnesses in `transaction-review.md` are addressed by the actual current code, not merely by a changed test expectation. This report supersedes that report's unresolved status for the hashes below.

Scope: read-only review of `canonical.rs`, `operation.rs`, `operation/tests/local_item_move.rs`, the existing strict validator and the relevant candidate publication/caller behavior. No Cargo, compilation, test execution, benchmark, timing, Git write, repository edit or child agent was run by this reviewer.

## Input witness and diagnostics

`operation.rs:631–647` receives the actual insertion split from the real checked executor and supplies it to `finish_item_move`. `insert_payload_at_edge` returns the existing same-axis parent's full NodeId after a roster insertion or the actual newly allocated wrapper NodeId. It does not recompute a hypothetical parent from the pre-detach topology. Center, tab-gap and the existing no-op branches return None. General and owned follow-up completion still ignore this receipt and use their existing full settlement.

`canonical.rs:230–245` checks that exact final insertion roster before any local topology rewrite. The extracted `validate_split_weights` has the same cardinality check, scalar checks, child-order f64 accumulation and unchanged `NORMALIZED_WEIGHT_TOLERANCE` as the original complete input validator. `validate_nodes` now calls that helper at the same point in its original arena order, before child existence/parent checks.

For the two-weight inner-edge witness, the receipt is the mutated target split. The positive scalar weights whose total is just outside tolerance fail the gate. The gate invokes the original `validate_rewrite_input`, so the observable error remains the full route's `WeightsNotNormalized` with the same node/sum and original deterministic priority. It does not introduce a command-level policy error, silently renormalize, or enlarge epsilon. If the narrow check and the complete diagnostic unexpectedly disagree, the existing TraversalInvariant error fails closed instead of returning a candidate.

The new regression `inner_edge_input_weight_sum_witness_matches_general_rejection_and_rolls_back` constructs and strictly validates the legal checked-Builder input from the original exact bits, asks the actual complete general route for its independent expected result, checks the expected error variant/node, runs the real public transaction, compares the entire error and asserts complete Workspace rollback and validity. It tests the original failure, not a raw-invalid workspace.

## Output witness and diagnostics

Every surviving source-path rewritten parent is appended to `output_splits` at `canonical.rs:347–350`. The inserted roster is also included even when no empty-source or outer repair occurs. Same-axis outer-wrapper repair passes through the same helper. After every required root/path rewrite finishes, lines 288–300 test only final still-live Split records for minimum arity, matching cardinality, finite positive scalars and the same aggregate bound.

This catches the 2000-child source-removal witness: the final retained source split is recorded by its actual rewrite, and the f32 normalizer's aggregate failure triggers the original complete strict validator. The public error is `InvalidOutput` with its full original ordered error collection, including `SplitWeightsNotNormalized`. The correction does not assume a successful `SplitWeight::normalize` proves strict aggregate validity.

`source_collapse_output_weight_sum_witness_matches_general_rejection_and_rolls_back` retains the exact accepted input bits, a checked/validated Builder workspace, a real Center move, an independent complete-general expected candidate, explicit expected strict error/node, full error equality and complete rollback. This directly covers the second distinct stage of the P1 finding.

## Coverage of all modified split shapes

The only numerical/topological split edits reachable from exactly one checked Item Move were traced:

| Actual edit | Frontier coverage |
| --- | --- |
| Center or TabGap into another tabs leaf with populated source | No split weights or topology change; no numerical frontier is needed. |
| Same-tabs Center/no-op self-edge/reorder | Existing no-op/reorder route returns no insertion split and changes no split weights. |
| Inner edge splitting an existing same-axis branch | Returns the actual mutated parent; input and final output checks include it. |
| Inner edge requiring a wrapper | Returns the new wrapper. The existing parent's child identity changes, but its cardinality/weights do not; wrapper and any affected source-path rewrites are checked. |
| Outer edge wrapper with either axis | New wrapper is recorded. A same-axis old root is flattened through recorded local rewrite. |
| Empty noncentral source leaf | Each current ancestor is repaired bottom-up. Every retained rewritten split is recorded; collapsed/deleted splits are retired. |
| Collapsed parent returning a split child | An unmodified returned child was already valid; a child produced earlier on the repair path was already recorded. No numerical result is lost when its parent disappears. |
| Flattened intermediate split | It is removed and skipped by the final live-record gate. Its retained parent is recorded after the actual normalize call. |
| Protected central source becoming empty | The empty central leaf is retained; no source weights change. The insertion frontier still covers the destination/wrapper. |
| Entire unprotected singleton source root retires | The emptied source record and its presentation are removed; target insertion/rewrites remain covered. |

No unrecorded new or rewritten split was found in those paths. The receipt is a private execution result inside the same exclusive candidate lifetime, not a reusable command proof or numeric eligibility container. It cannot authorize a later mutation. All lookups compare the full generational NodeId, and local normalization does not allocate a replacement into a retired slot.

## Ordering, transient records and canonical errors

Input checks occur before local rewrite, matching `canonicalize_data`'s input-before-normalization order. A changed input roster which is invalid is not allowed to become acceptable merely because an upcoming source collapse might normalize it.

Output checks are correctly deferred until the entire local rewrite is complete. A numerically invalid intermediate split may later be flattened away; rejecting it early would diverge from the full route, whose strict validation only observes the retained final graph. Conversely, if an outer/root normalization later fails with `InvalidNormalizedWeights`, that canonical failure must precede any accumulated numeric output defect. The code's final gate placement preserves that priority.

Distinct roots still repair in stable ascending RootId order. Within the source root, the current path is found after insertion, and only its affected descendants/ancestors need rewriting; all other original canonical subtrees are unchanged. The same exact `rewrite_split` arithmetic remains in use, so no f32 grouping, last-entry normalization or strict tolerance changed.

The known source surface is now pruned at `operation.rs:633–635`, before local finalization. This matches the full route's prune-before-canonicalization ordering. Therefore strict diagnostic fallback does not see a temporary vacant source presentation which the general route has already removed. On sealed input, no unrelated source can become vacant from this one Item Move, so the scoped prune remains sufficient.

On a failed final gate, the complete strict validator sees the same surviving topology/rows/MRU/root/presentation values as full canonicalization after sweep. Local repair directly removes the empty/collapsed/flattened records which that sweep would retire. No narrow first-bad-node error replaces the complete diagnostic collection. Short-circuiting `any` only decides whether to run the complete diagnostic pass; it does not determine public error order.

## Atomicity and unchanged successful work

The correction acts on the existing one-clone private candidate. Every newly added error returns before PreparedTransaction exists, and the public transaction publishes only successful preparation. Failed gates and diagnostic fallback cannot alter the caller's committed Workspace, version or native lifecycle state. No partial local program restarts under a fallback executor.

General multi-command, recovery and owned follow-up routes retain their original complete input/canonical/output checks; only the private execution return gained a split receipt which those routes discard. Authorization still happens before mutation in the original fused Move path. No policy, target-fingerprint, same-root target, command batch or no-op report behavior was changed by this fix.

`Vec::new` has no allocation for a populated-source Center move, and that path records/checks no split. Edge and empty-source paths check their actual frontier; full input or strict graph passes run only after a frontier failure. This is a source cost observation, not a measured claim. It preserves the closed-owner design rather than restoring unconditional global validation.

## Verification evidence and its limits

The reviewer read root's `combined-verified/checks-fmt,targeted.json`: format and the targeted nextest command both have exit code zero, with `sources_unchanged = true`. Root separately reported 32 targeted tests passed, including both numeric witnesses and the registered 10,000-depth regression. The reviewer recomputed hashes of the three fix files and the deep-test file; all match that root evidence exactly.

The targeted evidence is root's execution receipt, not a test run by this reviewer. Broader all-feature production qualification was still being run by root during this review. No final performance binary or measured ratio is certified here.

Two additional combinations remain useful future diagnostic locks, without constituting a current source finding: (1) an invalid source-path numeric output followed by a higher-root outer normalization failure, which must return the later canonical error rather than the earlier output defect; (2) a numerically invalid intermediate split subsequently flattened away, which must not be rejected solely because its retired ID was recorded. The current code's deferred final-live gate handles both by inspection. Do not create a new framework or delay a correctly scoped qualification merely to enumerate every combination.

## Reviewed exact source identity

- `crates/dockspace/src/canonical.rs`: `ba82d77b16aff98cf049a6cd0ced922aec7412cbd7443025579b5c204845628d` (matches root targeted receipt: true)
- `crates/dockspace/src/operation.rs`: `f89cc23fe15c7626e145ec2434853be6a01bedd3b3db07b1310650d9f312c586` (matches root targeted receipt: true)
- `crates/dockspace/src/operation/tests/local_item_move.rs`: `6448016ecf97272fb1432d26c6e540eb9077427c6613ef1deb89e0d2738307c5` (matches root targeted receipt: true)
- `crates/dockspace/src/layout/topology_tests.rs`: `1cbe351a68ff1554b987cee35a40826281e50ce406435ec1cb4ad117b0bd908d` (matches root targeted receipt: true)
